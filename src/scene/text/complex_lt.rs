//! CPU-side complex linetype tessellation.
//!
//! Walks the entity path and applies LT elements (dashes, gaps, dots, shapes),
//! returning one `WireModel` per continuous stroke.
//!
//! Coordinate convention: 2D entities live in the **XZ** plane (Y = elevation).
//! Shape X → along the linetype direction; Shape Y → perpendicular in XZ.

use crate::entities::text_support::resolve_dxf_special_chars;
use crate::io::linetypes::{ComplexLt, LtSegment};
use crate::scene::text::lff;
use crate::scene::model::wire_model::WireModel;

// ── Public entry point ────────────────────────────────────────────────────

/// Apply `lt` along `path_pts` (the entity's tessellated world-space strip).
///
/// Returns one `WireModel` per continuous stroke. Pass the entity's `name`,
/// `color`, `selected` flag, and `line_weight_px` so the WireModels inherit
/// the entity's visual properties.
/// Build the "A"-type segment list for one path: a solid begin/end dash of
/// length `align_end`, and between them the pattern's interior tiled to fill the
/// rest — starting at the element AFTER the first dash (so embedded text lands
/// in the same gaps the GPU dash shader phases to). Length-carrying elements are
/// truncated to fit exactly; zero-length glyphs (Dot / Text / Shape) pass
/// through at their interior position. Returns `None` if the interior would
/// exceed a sane repeat count (caller keeps the from-start tiling).
fn atype_segments(scaled: &[LtSeg], align_end: f32, total: f32) -> Option<Vec<LtSeg>> {
    let pat_len: f32 = scaled
        .iter()
        .map(|s| match s {
            LtSeg::Dash(l) | LtSeg::Space(l) => *l,
            _ => 0.0,
        })
        .sum();
    if pat_len < 1e-10 || scaled.len() < 2 {
        return None;
    }
    let ae = align_end.clamp(1e-4, total * 0.5);
    let interior = total - 2.0 * ae;
    if interior / pat_len > 50_000.0 {
        return None; // too many repeats to materialise — keep cycling walk
    }
    let mut segs: Vec<LtSeg> = Vec::new();
    segs.push(LtSeg::Dash(ae));
    if interior > 1e-6 {
        let mut filled = 0.0f32;
        let mut idx = 1usize; // element after the leading dash (the gap)
        while filled < interior - 1e-6 {
            match &scaled[idx % scaled.len()] {
                LtSeg::Dash(l) => {
                    let take = l.min(interior - filled);
                    segs.push(LtSeg::Dash(take));
                    filled += take;
                }
                LtSeg::Space(l) => {
                    let take = l.min(interior - filled);
                    segs.push(LtSeg::Space(take));
                    filled += take;
                }
                other => segs.push(other.clone()), // Dot / Text / Shape (0-length)
            }
            idx += 1;
        }
    }
    segs.push(LtSeg::Dash(ae));
    Some(segs)
}

pub fn apply_along(
    name: &str,
    path_pts: &[[f32; 3]],
    lt: &ComplexLt,
    scale: f32,
    color: [f32; 4],
    selected: bool,
    line_weight_px: f32,
    // Shared "A"-type reference length (the MLINE centre-line). `Some` end-aligns
    // the pattern: a solid begin/end dash whose length is derived from `ref_total`
    // (so every parallel element shares one interior phase and its dashes line up)
    // while this element still ends on a dash at its own endpoint. `None` tiles
    // the pattern from the start vertex (legacy phase, unchanged).
    ref_total: Option<f32>,
) -> Vec<WireModel> {
    if path_pts.len() < 2 || lt.segments.is_empty() {
        return vec![];
    }

    let scaled: Vec<LtSeg> = scale_segments(&lt.segments, scale);
    if scaled.is_empty() {
        return vec![];
    }

    let pattern_len: f32 = scaled
        .iter()
        .map(|s| match s {
            LtSeg::Dash(l) | LtSeg::Space(l) => *l,
            LtSeg::Dot | LtSeg::Shape { .. } | LtSeg::Text { .. } => 0.0,
        })
        .sum();
    if pattern_len < 1e-10 {
        return vec![];
    }

    // Guard against pattern blow-up. The per-segment walk below emits at least
    // one stroke vertex per pattern element, so the total vertex count scales
    // with `path_len / pattern_len`. On a large-extent drawing (e.g. a city/
    // street map) carrying a finely-scaled complex linetype, that ratio can
    // reach billions — the strokes Vec grows until the process is OOM-killed,
    // single-threaded, before the drawing ever finishes loading. Past a sane
    // repeat count the dashes are sub-pixel anyway, so render the base wire
    // solid: returning empty here makes the caller fall back to the solid base.
    const MAX_PATTERN_REPEATS: f32 = 1_000_000.0;
    let path_len: f32 = path_pts
        .windows(2)
        .map(|w| {
            let d = [w[1][0] - w[0][0], w[1][1] - w[0][1], w[1][2] - w[0][2]];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        })
        .sum();
    if path_len / pattern_len > MAX_PATTERN_REPEATS {
        return vec![];
    }

    // "A"-type end alignment (MLINE): with a shared reference length, replace the
    // from-start tiling with a solid begin/end dash and a phased interior so this
    // element lines up with its siblings and ends on a dash. Dash-first patterns
    // only; align_end is derived from `ref_total` (shared) but the trailing dash
    // lands at this element's own end (`path_len`).
    let atype_segs: Option<Vec<LtSeg>> = ref_total.and_then(|rt| {
        let a = match scaled.first() {
            Some(LtSeg::Dash(a)) if *a > 0.0 => *a,
            _ => return None,
        };
        if rt <= pattern_len || path_len <= pattern_len {
            return None;
        }
        let k = ((rt - a) / pattern_len).round().max(1.0);
        let align_end = (rt - k * pattern_len + a) * 0.5;
        atype_segments(&scaled, align_end, path_len)
    });
    let atype_on = atype_segs.is_some();
    let walk_segs: &[LtSeg] = atype_segs.as_deref().unwrap_or(&scaled);

    let mut strokes: Vec<Vec<[f32; 3]>> = Vec::new();
    let mut cur_stroke: Vec<[f32; 3]> = Vec::new();
    // Embedded linetype text (LtSeg::Text) renders as SDF glyph quads collected
    // here; emitted as one extra text-carrying WireModel at the end.
    let mut text_verts: Vec<crate::scene::pipeline::text_gpu::TextVertex> = Vec::new();

    let mut elem_idx: usize = 0;
    let mut elem_consumed: f32 = 0.0;

    'path: for i in 0..path_pts.len() - 1 {
        let ps = path_pts[i];
        let pe = path_pts[i + 1];

        let dx = pe[0] - ps[0];
        let dy = pe[1] - ps[1];
        let dz = pe[2] - ps[2];
        let seg_len = (dx * dx + dy * dy + dz * dz).sqrt();
        if seg_len < 1e-10 {
            continue;
        }

        let fwd = [dx / seg_len, dy / seg_len, dz / seg_len];
        let perp = [-fwd[1], fwd[0], fwd[2]];

        let mut pos = 0.0f32;

        while pos < seg_len - 1e-6 {
            let idx = if atype_on {
                // A-type walks a finite, path-sized segment list once (no cycling);
                // once consumed the whole path is drawn.
                if elem_idx >= walk_segs.len() {
                    break 'path;
                }
                elem_idx
            } else {
                elem_idx % walk_segs.len()
            };

            match &walk_segs[idx] {
                LtSeg::Dash(dash_len) => {
                    let remaining_dash = dash_len - elem_consumed;
                    let remaining_seg = seg_len - pos;
                    let advance = remaining_dash.min(remaining_seg);

                    let p_start = lerp(ps, pe, pos / seg_len);
                    let p_end = lerp(ps, pe, (pos + advance) / seg_len);

                    if cur_stroke.is_empty() {
                        cur_stroke.push(p_start);
                    }
                    cur_stroke.push(p_end);

                    pos += advance;
                    elem_consumed += advance;
                    if (elem_consumed - dash_len).abs() < 1e-6 {
                        elem_idx += 1;
                        elem_consumed = 0.0;
                    }
                }

                LtSeg::Space(space_len) => {
                    let remaining = space_len - elem_consumed;
                    let advance = remaining.min(seg_len - pos);

                    flush(&mut cur_stroke, &mut strokes);

                    pos += advance;
                    elem_consumed += advance;
                    if (elem_consumed - space_len).abs() < 1e-6 {
                        elem_idx += 1;
                        elem_consumed = 0.0;
                    }
                }

                LtSeg::Dot => {
                    flush(&mut cur_stroke, &mut strokes);
                    let p = lerp(ps, pe, pos / seg_len);
                    strokes.push(vec![p, p]);
                    elem_idx += 1;
                    elem_consumed = 0.0;
                    if pos < 1e-6 {
                        break;
                    }
                }

                LtSeg::Shape {
                    name: sh_name,
                    shx_file,
                    number,
                    x,
                    y,
                    scale: sh_scale,
                    rot_deg,
                } => {
                    flush(&mut cur_stroke, &mut strokes);

                    let insert = lerp(ps, pe, pos / seg_len);
                    let insert = offset_pt(insert, fwd, perp, *x, *y);

                    let shape_strokes = emit_shape(
                        sh_name, shx_file, *number, insert, fwd, perp, *sh_scale, *rot_deg,
                    );
                    strokes.extend(shape_strokes);

                    elem_idx += 1;
                    elem_consumed = 0.0;
                    if pos < 1e-6 {
                        break;
                    }
                }
                LtSeg::Text {
                    text,
                    style,
                    x,
                    y,
                    scale: tx_scale,
                    rot_deg,
                } => {
                    flush(&mut cur_stroke, &mut strokes);

                    let insert = lerp(ps, pe, pos / seg_len);
                    let insert = offset_pt(insert, fwd, perp, *x, *y);
                    let fwd_angle = fwd[1].atan2(fwd[0]) + rot_deg.to_radians();
                    let resolved = resolve_dxf_special_chars(text);
                    // SDF: glyph quads at the insert point (rotation baked in by
                    // layout_glyph_quads), collected for the text wire.
                    if let Ok(mut atlas) = crate::scene::text::sdf_atlas::text_atlas().lock() {
                        let quads = crate::scene::text::glyph_quads::layout_glyph_quads(
                            &mut atlas,
                            *tx_scale,
                            fwd_angle,
                            1.0, // width_factor
                            0.0, // oblique
                            // tracking = 1.0 applies the font's natural
                            // letter_spacing; 0.0 zeroes it and cramps the glyphs
                            // (every other text path passes 1.0).
                            1.0,
                            style,
                            false,
                            &resolved,
                        );
                        crate::scene::pipeline::text_gpu::push_glyph_vertices(
                            &mut text_verts,
                            &quads,
                            [insert[0] as f64, insert[1] as f64, insert[2] as f64],
                            1.0,
                            color,
                            0.0,
                        );
                    }

                    elem_idx += 1;
                    elem_consumed = 0.0;
                    if pos < 1e-6 {
                        break;
                    }
                }
            }
        }
    }

    flush(&mut cur_stroke, &mut strokes);

    let mut out: Vec<WireModel> = strokes
        .into_iter()
        .filter(|s| s.len() >= 2)
        .map(|pts| WireModel {
            point_marker: None,
            taper_widths: Vec::new(),
            world_width: 0.0,
            depth_override: None,
            display_visible: true,
            plot_visible: true,
            fill_is_3d: false,
            fill_is_2d_solid: false,
            render_instance: None,
            pick_tris: Vec::new(),
            pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
            name: name.to_string(),
            points: pts,
            points_low: Vec::new(),
            color,
            selected,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px,
            snap_pts: vec![],
            tangent_geoms: vec![],
            aci: 0,
            key_vertices: vec![],
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            fill_tris: vec![],
            fill_tris_low: Vec::new(),
        })
        .collect();

    // Embedded linetype text (SDF): one extra wire carrying the glyph quads,
    // with a glyph-bounds AABB (f64 accumulate → f32) so it stays precise at
    // UTM scale. Empty points so it doesn't add stroke geometry.
    if !text_verts.is_empty() {
        let (mut nx, mut ny, mut xx, mut xy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for v in &text_verts {
            let x = v.pos[0] as f64 + v.pos_low[0] as f64;
            let y = v.pos[1] as f64 + v.pos_low[1] as f64;
            nx = nx.min(x);
            xx = xx.max(x);
            ny = ny.min(y);
            xy = xy.max(y);
        }
        out.push(WireModel {
            point_marker: None,
            taper_widths: Vec::new(),
            world_width: 0.0,
            depth_override: None,
            display_visible: true,
            plot_visible: true,
            fill_is_3d: false,
            fill_is_2d_solid: false,
            render_instance: None,
            pick_tris: Vec::new(),
            pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts,
            name: name.to_string(),
            points: Vec::new(),
            points_low: Vec::new(),
            color,
            selected,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px,
            snap_pts: vec![],
            tangent_geoms: vec![],
            aci: 0,
            key_vertices: vec![],
            aabb: [nx as f32, ny as f32, xx as f32, xy as f32],
            plinegen: true,
            fill_tris: vec![],
            fill_tris_low: Vec::new(),
        });
    }
    out
}

// ── Helpers ───────────────────────────────────────────────────────────────

#[derive(Clone)]
enum LtSeg {
    Dash(f32),
    Space(f32),
    Dot,
    Shape {
        name: String,
        shx_file: String,
        number: u16,
        x: f32,
        y: f32,
        scale: f32,
        rot_deg: f32,
    },
    Text {
        text: String,
        style: String,
        x: f32,
        y: f32,
        scale: f32,
        rot_deg: f32,
    },
}

fn scale_segments(segs: &[LtSegment], scale: f32) -> Vec<LtSeg> {
    segs.iter()
        .map(|s| match s {
            LtSegment::Dash(l) => LtSeg::Dash(l * scale),
            LtSegment::Space(l) => LtSeg::Space(l * scale),
            LtSegment::Dot => LtSeg::Dot,
            LtSegment::Shape {
                name,
                shx_file,
                number,
                x,
                y,
                scale: sh_scale,
                rot_deg,
            } => LtSeg::Shape {
                name: name.clone(),
                shx_file: shx_file.clone(),
                number: *number,
                x: x * scale,
                y: y * scale,
                scale: *sh_scale * scale,
                rot_deg: *rot_deg,
            },
            LtSegment::Text {
                text,
                style,
                x,
                y,
                scale: tx_scale,
                rot_deg,
            } => LtSeg::Text {
                text: text.clone(),
                style: style.clone(),
                x: x * scale,
                y: y * scale,
                scale: *tx_scale * scale,
                rot_deg: *rot_deg,
            },
        })
        .collect()
}

fn flush(cur: &mut Vec<[f32; 3]>, strokes: &mut Vec<Vec<[f32; 3]>>) {
    if !cur.is_empty() {
        strokes.push(std::mem::take(cur));
    }
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn offset_pt(pt: [f32; 3], fwd: [f32; 3], perp: [f32; 3], dx: f32, dy: f32) -> [f32; 3] {
    [
        pt[0] + fwd[0] * dx + perp[0] * dy,
        pt[1] + fwd[1] * dx + perp[1] * dy,
        pt[2] + fwd[2] * dx + perp[2] * dy,
    ]
}

/// Transform a linetype shape into world-space strokes at the pen position:
/// the real .SHX glyph when the segment carries a resolved shape file (by
/// number for document linetypes, by name for `.lin` references), falling
/// back to the converted `ltypeshp` LFF substitute set.
fn emit_shape(
    name: &str,
    shx_file: &str,
    number: u16,
    insert: [f32; 3],
    fwd: [f32; 3],
    perp: [f32; 3],
    scale: f32,
    rot_deg: f32,
) -> Vec<Vec<[f32; 3]>> {
    if !shx_file.is_empty() {
        let polys = if number > 0 {
            crate::scene::text::shx::shape_polylines(shx_file, number)
        } else {
            crate::scene::text::shx::shape_polylines_by_name(shx_file, name)
        };
        if let Some(polys) = polys {
            let rot_r = rot_deg.to_radians();
            let (cos_r, sin_r) = (rot_r.cos(), rot_r.sin());
            return polys
                .iter()
                .map(|stroke| {
                    stroke
                        .iter()
                        .map(|&[lx, ly]| {
                            let scaled_x = lx as f32 * scale;
                            let scaled_y = ly as f32 * scale;
                            let along_fwd = cos_r * scaled_x - sin_r * scaled_y;
                            let along_perp = sin_r * scaled_x + cos_r * scaled_y;
                            [
                                insert[0] + fwd[0] * along_fwd + perp[0] * along_perp,
                                insert[1] + fwd[1] * along_fwd + perp[1] * along_perp,
                                insert[2] + fwd[2] * along_fwd + perp[2] * along_perp,
                            ]
                        })
                        .collect()
                })
                .collect();
        }
    }
    let shape = match lff::shape(name) {
        Some(s) => s,
        None => return vec![],
    };

    let rot_r = rot_deg.to_radians();
    let (cos_r, sin_r) = (rot_r.cos(), rot_r.sin());

    shape
        .strokes
        .iter()
        .map(|stroke| {
            stroke
                .iter()
                .map(|&[lx, ly]| {
                    let scaled_x = lx * scale;
                    let scaled_y = ly * scale;
                    // Rotate in the shape's local frame, then place along the
                    // line tangent (fwd) and perpendicular (perp).
                    let along_fwd = cos_r * scaled_x - sin_r * scaled_y;
                    let along_perp = sin_r * scaled_x + cos_r * scaled_y;
                    [
                        insert[0] + fwd[0] * along_fwd + perp[0] * along_perp,
                        insert[1] + fwd[1] * along_fwd + perp[1] * along_perp,
                        insert[2] + fwd[2] * along_fwd + perp[2] * along_perp,
                    ]
                })
                .collect()
        })
        .collect()
}
