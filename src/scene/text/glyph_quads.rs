//! Phase 2a of the text-shader initiative: lay a text run out as per-glyph
//! quads over the SDF atlas, instead of tessellating glyph outlines into wire
//! strokes.
//!
//! Each visible glyph becomes one quad whose corners are the glyph's atlas
//! `plane` rectangle (ink bbox + SDF spread, in 9-unit glyph space) run through
//! the exact same run transform the stroke path uses (`tessellate_text_run`):
//! scale = height/9, width factor, oblique skew, rotation. Corners come out in
//! run-local 2-D (origin at `[0, 0]`) — the same space `TextStroke.strokes`
//! use — so the downstream path (annotation scale, f64 origin, double-single
//! split) handles them identically. The paired atlas UV rect lets the fragment
//! shader sample the glyph's SDF tile.
//!
//! Layout here is per-character advance (LFF exact; TTF without shaping). TTF
//! shaping (kerning/ligatures via `shape_run`) is a later refinement — it needs
//! per-glyph offsets within a shaped run, which this cut does not yet apply.

// Not yet wired into the render path (Phase 2b: the text quad pipeline).
#![allow(dead_code)]

use crate::scene::text::font_face::Face;
use crate::scene::text::sdf_atlas::GlyphAtlas;

/// One glyph placed as a quad in run-local 2-D space (origin at `[0, 0]`).
#[derive(Clone, Copy, Debug)]
pub struct GlyphQuad {
    /// Corners, CCW: bottom-left, bottom-right, top-right, top-left.
    pub corners: [[f32; 2]; 4],
    /// Atlas UV of the tile (top-left / bottom-right).
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
}

/// Lay `text` out as per-glyph quads. Mirrors `tessellate_text_run`'s transform
/// and per-character advance so the quads land exactly where the strokes would.
/// Whitespace and glyphs with no ink advance the pen but emit no quad.
#[allow(clippy::too_many_arguments)]
pub fn layout_glyph_quads(
    atlas: &mut GlyphAtlas,
    height: f32,
    rotation: f32,
    width_factor: f32,
    oblique_angle: f32,
    tracking: f32,
    font_name: &str,
    bold: bool,
    text: &str,
) -> Vec<GlyphQuad> {
    if text.is_empty() || height <= 0.0 {
        return vec![];
    }

    let scale = height / 9.0;
    let wf = if width_factor < 0.0 {
        width_factor.clamp(-100.0, -0.01)
    } else {
        width_factor.clamp(0.01, 100.0)
    };
    let ob = oblique_angle.tan();
    let (cos_r, sin_r) = (rotation.cos(), rotation.sin());

    // Glyph-space (gx, gy) at pen position cx -> run-local 2-D (origin [0, 0]).
    // Identical to `tessellate_text_run::xform` with `origin = [0, 0]`.
    let xform = |gx: f32, gy: f32, cx: f32| -> [f32; 2] {
        let sx = (cx + gx) * scale * wf + gy * scale * ob;
        let sy = gy * scale;
        [sx * cos_r - sy * sin_r, sx * sin_r + sy * cos_r]
    };

    use crate::scene::text::lff::{tokenize_run, Deco, Op, Tok};

    // Decoration line positions (glyph units) and half-thickness — mirror the
    // stroke tessellator's constants so SDF decorations align with strokes.
    const UNDER_Y: f32 = -1.5;
    const OVER_Y: f32 = 10.5;
    const STRIKE_Y: f32 = 4.5;
    const DECO_HALF: f32 = 0.35;

    let solid = atlas.solid_uv();
    let face = Face::resolve(font_name);
    let mut cursor_x = 0.0f32;
    let mut quads = Vec::new();
    // Open decoration runs (pen x where each turned on) and the spans to draw.
    let mut under: Option<f32> = None;
    let mut over: Option<f32> = None;
    let mut strike: Option<f32> = None;
    let mut decos: Vec<(f32, f32, f32)> = Vec::new(); // (start_x, end_x, y)

    // Same token walk as `tessellate_text_run` (LFF branch): DXF `%%` specials
    // resolve to glyphs, spaces/missing advance the pen, decoration toggles
    // open/close underline / overline / strikethrough runs.
    for tok in tokenize_run(text) {
        let ch = match tok {
            Tok::Glyph(c) => c,
            Tok::Space => {
                cursor_x += face.word_spacing();
                continue;
            }
            Tok::Missing => {
                cursor_x += 6.0 + face.letter_spacing() * tracking;
                continue;
            }
            Tok::Deco(deco, op) => {
                let (slot, y) = match deco {
                    Deco::Under => (&mut under, UNDER_Y),
                    Deco::Over => (&mut over, OVER_Y),
                    Deco::Strike => (&mut strike, STRIKE_Y),
                };
                match op {
                    Op::On => {
                        if slot.is_none() {
                            *slot = Some(cursor_x);
                        }
                    }
                    Op::Off => {
                        if let Some(s) = slot.take() {
                            decos.push((s, cursor_x, y));
                        }
                    }
                    Op::Toggle => {
                        *slot = match slot.take() {
                            Some(s) => {
                                decos.push((s, cursor_x, y));
                                None
                            }
                            None => Some(cursor_x),
                        };
                    }
                }
                continue;
            }
        };
        match atlas.get_or_insert(font_name, ch, bold) {
            Some(e) => {
                let (lo, hi) = (e.plane_min, e.plane_max);
                quads.push(GlyphQuad {
                    corners: [
                        xform(lo[0], lo[1], cursor_x),
                        xform(hi[0], lo[1], cursor_x),
                        xform(hi[0], hi[1], cursor_x),
                        xform(lo[0], hi[1], cursor_x),
                    ],
                    uv_min: e.uv_min,
                    uv_max: e.uv_max,
                });
                cursor_x += e.advance + face.letter_spacing() * tracking;
            }
            None => {
                // No ink (whitespace glyph) or atlas full: advance only.
                let adv = face.glyph(ch).map(|g| g.advance).unwrap_or(6.0);
                cursor_x += adv + face.letter_spacing() * tracking;
            }
        }
    }

    // Any decoration still open at the end runs to the final pen position.
    for (slot, y) in [(under, UNDER_Y), (over, OVER_Y), (strike, STRIKE_Y)] {
        if let Some(s) = slot {
            decos.push((s, cursor_x, y));
        }
    }
    // Decoration lines: thin quads sampling the atlas's solid texel so the SDF
    // shader fills them solid in the run colour.
    for (s, e, y) in decos {
        quads.push(GlyphQuad {
            corners: [
                xform(s, y - DECO_HALF, 0.0),
                xform(e, y - DECO_HALF, 0.0),
                xform(e, y + DECO_HALF, 0.0),
                xform(s, y + DECO_HALF, 0.0),
            ],
            uv_min: solid,
            uv_max: solid,
        });
    }

    quads
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn center(q: &GlyphQuad) -> [f32; 2] {
        let mut c = [0.0f32; 2];
        for p in &q.corners {
            c[0] += p[0] * 0.25;
            c[1] += p[1] * 0.25;
        }
        c
    }

    // Uses the embedded "txt" stroke font (no system fonts needed).
    #[test]
    fn glyphs_advance_rightward() {
        let mut atlas = GlyphAtlas::new(512, 512);
        let quads = layout_glyph_quads(&mut atlas, 10.0, 0.0, 1.0, 0.0, 1.0, "txt", false, "AA");
        assert_eq!(quads.len(), 2, "two inked glyphs -> two quads");
        assert!(
            center(&quads[1])[0] > center(&quads[0])[0],
            "second glyph is to the right of the first"
        );
        for q in &quads {
            for p in &q.corners {
                assert!(p[0].is_finite() && p[1].is_finite());
            }
            assert!(q.uv_min[0] >= 0.0 && q.uv_max[0] <= 1.0);
            assert!(q.uv_min[1] >= 0.0 && q.uv_max[1] <= 1.0);
        }
    }

    #[test]
    fn space_emits_no_quad_but_advances() {
        let mut atlas = GlyphAtlas::new(512, 512);
        let ab = layout_glyph_quads(&mut atlas, 10.0, 0.0, 1.0, 0.0, 1.0, "txt", false, "AA");
        let a_sp_a = layout_glyph_quads(&mut atlas, 10.0, 0.0, 1.0, 0.0, 1.0, "txt", false, "A A");
        assert_eq!(a_sp_a.len(), 2, "space produces no quad");
        assert!(
            center(&a_sp_a[1])[0] > center(&ab[1])[0],
            "the space widens the gap to the second glyph"
        );
    }

    #[test]
    fn rotation_maps_x_advance_to_y() {
        let mut atlas = GlyphAtlas::new(512, 512);
        // Use the baseline advance direction (glyph0 -> glyph1), which is +x when
        // flat and swings to +y after a 90° turn — independent of where a single
        // glyph's box centre sits within the cap height.
        let flat = layout_glyph_quads(&mut atlas, 10.0, 0.0, 1.0, 0.0, 1.0, "txt", false, "AA");
        let turned = layout_glyph_quads(
            &mut atlas,
            10.0,
            std::f32::consts::FRAC_PI_2,
            1.0,
            0.0,
            1.0,
            "txt",
            false,
            "AA",
        );
        let adv = |qs: &[GlyphQuad]| {
            let (a, b) = (center(&qs[0]), center(&qs[1]));
            [b[0] - a[0], b[1] - a[1]]
        };
        let df = adv(&flat);
        let dt = adv(&turned);
        assert!(df[0] > df[1].abs(), "unrotated advance is along +x");
        assert!(dt[1] > dt[0].abs(), "90° advance swings to +y");
    }

    #[test]
    fn taller_text_makes_bigger_quads() {
        let mut atlas = GlyphAtlas::new(512, 512);
        let small = layout_glyph_quads(&mut atlas, 10.0, 0.0, 1.0, 0.0, 1.0, "txt", false, "A");
        let big = layout_glyph_quads(&mut atlas, 20.0, 0.0, 1.0, 0.0, 1.0, "txt", false, "A");
        let extent = |q: &GlyphQuad| {
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            for p in &q.corners {
                lo[0] = lo[0].min(p[0]);
                lo[1] = lo[1].min(p[1]);
                hi[0] = hi[0].max(p[0]);
                hi[1] = hi[1].max(p[1]);
            }
            (hi[1] - lo[1]).max(hi[0] - lo[0])
        };
        let r = extent(&big[0]) / extent(&small[0]);
        assert!(
            (r - 2.0).abs() < 0.15,
            "double height ~ double quad extent, got {r}"
        );
    }

    #[test]
    fn underline_emits_an_extra_solid_quad() {
        let mut atlas = GlyphAtlas::new(512, 512);
        let plain = layout_glyph_quads(&mut atlas, 10.0, 0.0, 1.0, 0.0, 1.0, "txt", false, "AB");
        // `\L…\l` underlines the run — same glyph quads plus one decoration quad.
        let under = layout_glyph_quads(
            &mut atlas, 10.0, 0.0, 1.0, 0.0, 1.0, "txt", false, "\\LAB\\l",
        );
        assert_eq!(under.len(), plain.len() + 1, "underline adds one quad");
        // The decoration quad samples the atlas's solid texel (degenerate UV).
        let deco = under.last().unwrap();
        assert_eq!(
            deco.uv_min, deco.uv_max,
            "decoration quad uses the solid UV"
        );
        assert_eq!(deco.uv_min, atlas.solid_uv());
    }
}
