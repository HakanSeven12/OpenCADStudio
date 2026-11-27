// Metafile text → canvas, using the CAD text renderer's glyph outline cache.
//
// Glyph outlines arrive from `ttf_glyph` pre-triangulated in a 9-unit
// cap-height space with the baseline at y = 0 (y-up); here they are scaled by
// the font's device pixel size, flipped to the canvas's y-down space, rotated
// by the font escapement, and filled.

use super::raster::Canvas;
use super::{Dc, Font};
use crate::text::{sysfont, ttf_glyph};

/// Per-face metrics needed to place metafile text: cap-height and ascent as
/// fractions of the em. Falls back to typical Latin ratios when the face (or
/// the whole family) is unavailable.
fn face_ratios(family: &str) -> (f32, f32) {
    sysfont::with_face_data(family, |data, index| {
        let face = ttf_parser::Face::parse(data, index).ok()?;
        let upem = face.units_per_em() as f32;
        let cap = face
            .capital_height()
            .filter(|&c| c > 0)
            .map(|c| c as f32 / upem)
            .unwrap_or(0.7);
        let asc = face.ascender() as f32 / upem;
        Some((cap, asc.max(0.5)))
    })
    .flatten()
    .unwrap_or((0.7, 0.9))
}

/// Resolve a LOGFONT facename to an installed family, with sane fallbacks.
fn resolve_family(facename: &str) -> Option<String> {
    if !facename.is_empty() {
        if let Some(f) = sysfont::canonical_family_name(facename) {
            return Some(f);
        }
    }
    for cand in ["Arial", "Liberation Sans", "DejaVu Sans", "Noto Sans"] {
        if let Some(f) = sysfont::canonical_family_name(cand) {
            return Some(f);
        }
    }
    sysfont::families().first().cloned()
}

/// TA_* horizontal / vertical alignment split.
struct Align {
    /// 0 = left, 1 = center, 2 = right.
    h: u8,
    /// 0 = top, 1 = baseline, 2 = bottom.
    v: u8,
    update_cp: bool,
}

fn split_align(ta: u32) -> Align {
    let h = if ta & 6 == 6 {
        1
    } else if ta & 2 != 0 {
        2
    } else {
        0
    };
    let v = if ta & 24 == 24 {
        1
    } else if ta & 8 != 0 {
        2
    } else {
        0
    };
    Align {
        h,
        v,
        update_cp: ta & 1 != 0,
    }
}

/// Draw one metafile text run.
///
/// `xr`, `yr` — the logical-space reference point. `dx` — optional per-char
/// logical advances (metafiles record them so playback matches the recorder's
/// font metrics exactly; without them the run is shaped fresh).
#[allow(clippy::too_many_arguments)]
pub fn draw_text(
    canvas: &mut Canvas,
    dc: &mut Dc,
    xr: f32,
    yr: f32,
    text: &str,
    dx: Option<&[f32]>,
    opaque_rect: Option<[f32; 4]>,
) {
    if text.is_empty() {
        return;
    }
    let font = dc.font.clone();
    let Some(family) = resolve_family(&font.facename) else {
        return;
    };
    let (cap_ratio, ascent_ratio) = face_ratios(&family);

    // Font pixel size on the canvas. Negative LOGFONT height = em size,
    // positive = cell height (em + internal leading ≈ em × 1.15).
    let em_log = if font.height < 0.0 {
        -font.height
    } else if font.height > 0.0 {
        font.height / 1.15
    } else {
        12.0
    };
    let em_px = em_log * dc.scale_y();
    if em_px < 0.5 {
        return;
    }
    // Glyph 9-unit cap space → canvas pixels.
    let s = em_px * cap_ratio / 9.0;
    let ascent_px = em_px * ascent_ratio;

    let align = split_align(dc.text_align);
    let (rx, ry) = if align.update_cp {
        (dc.pos.0, dc.pos.1)
    } else {
        (xr, yr)
    };

    // Fill the opaque background rectangle first (logical coords).
    if let Some([x0, y0, x1, y1]) = opaque_rect {
        let a = dc.to_canvas(x0, y0);
        let b = dc.to_canvas(x1, y1);
        let bk = dc.bk_color;
        canvas.fill_rect(
            a[0].min(b[0]),
            a[1].min(b[1]),
            a[0].max(b[0]),
            a[1].max(b[1]),
            bk,
        );
    }

    // Collect positioned glyph triangles in run-local pixel space (x right,
    // y down, origin at the run's left baseline start).
    let mut tris: Vec<[f32; 2]> = Vec::new();
    let run_advance_px: f32;
    let log_to_px = dc.scale_avg();

    if let Some(dx) = dx {
        // Recorded per-char advances: place each char's glyph independently.
        let mut pen_x = 0.0_f32;
        for (i, ch) in text.chars().enumerate() {
            let g = ttf_glyph::glyph(&family, ch).or_else(|| ttf_glyph::fallback_glyph(ch));
            if let Some(g) = g {
                for v in &g.fill_tris {
                    tris.push([pen_x + v[0] * s, -v[1] * s]);
                }
            }
            pen_x += dx.get(i).copied().unwrap_or(0.0) * log_to_px;
        }
        run_advance_px = pen_x;
    } else if let Some(run) = ttf_glyph::shape_run(&family, text) {
        for g in &run.glyphs {
            for v in &g.fill_tris {
                tris.push([v[0] * s, -v[1] * s]);
            }
        }
        run_advance_px = run.advance * s;
    } else {
        // No shaping available (e.g. web build): naive per-char advances.
        let mut pen_x = 0.0_f32;
        for ch in text.chars() {
            let g = ttf_glyph::glyph(&family, ch).or_else(|| ttf_glyph::fallback_glyph(ch));
            if let Some(g) = g {
                for v in &g.fill_tris {
                    tris.push([pen_x + v[0] * s, -v[1] * s]);
                }
                pen_x += g.advance * s;
            } else {
                pen_x += em_px * 0.5;
            }
        }
        run_advance_px = pen_x;
    }

    // Alignment offsets in run-local space.
    let x_off = match align.h {
        1 => -run_advance_px * 0.5,
        2 => -run_advance_px,
        _ => 0.0,
    };
    let y_off = match align.v {
        1 => 0.0,           // reference is the baseline
        2 => -em_px * 0.25, // bottom ≈ baseline + descent
        _ => ascent_px,     // top: baseline sits one ascent below
    };

    // Rotate by escapement (0.1° CCW in logical space = CW in y-down canvas),
    // then translate to the canvas-space reference point.
    let ref_px = dc.to_canvas(rx, ry);
    let ang = font.escapement / 10.0 * std::f32::consts::PI / 180.0;
    let (sin, cos) = (-ang).sin_cos();
    let color = dc.text_color;
    for v in &mut tris {
        let x = v[0] + x_off;
        let y = v[1] + y_off;
        *v = [ref_px[0] + x * cos - y * sin, ref_px[1] + x * sin + y * cos];
    }
    canvas.fill_tris(&tris, color);

    if align.update_cp {
        // Advance the current position along the run direction (logical).
        let adv_log = run_advance_px / log_to_px.max(1e-12);
        dc.pos.0 += adv_log * (font.escapement / 10.0).to_radians().cos();
        dc.pos.1 -= adv_log * (font.escapement / 10.0).to_radians().sin();
    }
}

/// Decode a WMF ANSI string per the current font's charset. Only the Turkish
/// code page (162) gets explicit treatment beyond Windows-1252.
pub fn decode_ansi(bytes: &[u8], font: &Font) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x80 => '€',
            0x82 => '‚',
            0x84 => '„',
            0x85 => '…',
            0x91 => '\u{2018}',
            0x92 => '\u{2019}',
            0x93 => '“',
            0x94 => '”',
            0x95 => '•',
            0x96 => '–',
            0x97 => '—',
            0xD0 if font.charset == 162 => 'Ğ',
            0xDD if font.charset == 162 => 'İ',
            0xDE if font.charset == 162 => 'Ş',
            0xF0 if font.charset == 162 => 'ğ',
            0xFD if font.charset == 162 => 'ı',
            0xFE if font.charset == 162 => 'ş',
            b => b as char,
        })
        .collect()
}
