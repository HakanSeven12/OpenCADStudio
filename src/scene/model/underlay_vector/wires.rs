//! A DWF sheet or DGN model drawn as vector wires (lines and flat fills)
//! in world space, so it stays sharp at every zoom as the reference draws
//! it, instead of as a raster rescaled to the view.

use std::collections::HashMap;

use codec::entities::{Underlay, UnderlayDefinition, UnderlayDisplayFlags, UnderlayType};

use super::{model, sheet_without, Backdrop, Segment, Sheet};
use crate::scene::model::wire_model::WireModel;

/// Wires for the underlay's content, one per colour and kind (lines, fills),
/// named after the underlay so they draw in its place. They are drawn and
/// plotted but never picked or snapped (the frame selects the underlay; the
/// underlay's own snap geometry serves object snaps).
pub(crate) fn display_wires(
    u: &Underlay,
    def: &UnderlayDefinition,
    name: &str,
    background: [f32; 4],
) -> Vec<WireModel> {
    if def.underlay_type == UnderlayType::Pdf || def.unloaded || !u.flags.contains(UnderlayDisplayFlags::ON) {
        return Vec::new();
    }
    let page = crate::entities::underlay::page_of(def);
    let hidden = crate::scene::model::pdf_layers::hidden_layers(u);
    let Some(sheet) = sheet_without(def.underlay_type, &def.file_path, page, &hidden) else {
        return Vec::new();
    };
    let adjust = crate::scene::model::image_model::underlay_adjust(u, def, background);
    let backdrop = Backdrop::of([background[0], background[1], background[2]]);
    // A DGN model's fade is in its colours; a DWF sheet's blends it with
    // what is under it.
    let alpha = if def.underlay_type == UnderlayType::Dgn { 1.0 } else { 1.0 - u.fade.min(100) as f32 / 100.0 };
    let shown = |c: [u8; 3]| -> [f32; 4] {
        let c = if sheet.table_colors { model::dgn_table_color(c, backdrop.max_channel, backdrop.light) } else { c };
        let rgb = crate::scene::model::pdf_raster::adjust_rgb([c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0], &adjust);
        [rgb[0], rgb[1], rgb[2], alpha]
    };
    let world = |p: [f64; 2]| crate::entities::underlay::local_to_world(u, p);
    let tolerance = curve_tolerance(&sheet);
    let scale = u.x_scale.abs().max(u.y_scale.abs());

    // Lines per (colour, width), fills per colour, each in first-drawn order.
    let key = |c: [f32; 4]| c.map(f32::to_bits);
    let mut lines: Vec<([f32; 4], f64, Vec<[f64; 3]>)> = Vec::new();
    let mut line_at: HashMap<([u32; 4], u64), usize> = HashMap::new();
    let mut fills: Vec<([f32; 4], Vec<[f64; 3]>)> = Vec::new();
    let mut fill_at: HashMap<[u32; 4], usize> = HashMap::new();
    for path in &sheet.paths {
        if let Some(c) = path.fill {
            let color = shown(c);
            let rings: Vec<Vec<[f64; 2]>> = path
                .subpaths
                .iter()
                .map(|sp| polyline(&sp.segments, tolerance))
                .filter(|r| r.len() >= 3)
                .collect();
            if !rings.is_empty() {
                let (points, triangles) = kernel::geom2d::triangulate_rings(&rings);
                let at = *fill_at.entry(key(color)).or_insert_with(|| {
                    fills.push((color, Vec::new()));
                    fills.len() - 1
                });
                for triangle in triangles {
                    for index in triangle {
                        if let Some(&p) = points.get(index) {
                            fills[at].1.push(world(p));
                        }
                    }
                }
            }
        }
        if let Some((c, width)) = path.stroke {
            let color = shown(c);
            let at = *line_at.entry((key(color), width.to_bits())).or_insert_with(|| {
                lines.push((color, width, Vec::new()));
                lines.len() - 1
            });
            for sp in &path.subpaths {
                let mut pts = polyline(&sp.segments, tolerance);
                if sp.closed && pts.len() >= 2 && pts.first() != pts.last() {
                    pts.push(pts[0]);
                }
                if pts.len() < 2 {
                    continue;
                }
                let run = &mut lines[at].2;
                if !run.is_empty() {
                    run.push([f64::NAN; 3]);
                }
                run.extend(pts.into_iter().map(world));
            }
        }
    }

    let mut wires = Vec::new();
    for (color, tris) in fills {
        let (fp, fp_low) = crate::scene::convert::tessellate::points_to_ds(tris);
        let mut wire = content_wire(name, Vec::new(), color);
        wire.fill_tris = fp;
        wire.fill_tris_low = fp_low;
        wires.push(wire);
    }
    for (color, width, points) in lines {
        let (lp, lp_low) = crate::scene::convert::tessellate::points_to_ds(points);
        let mut wire = content_wire(name, lp, color);
        wire.points_low = lp_low;
        if width < 0.0 {
            // A width in pixels (DGN line weights are not shown).
            wire.set_fixed_screen_width((-width) as f32);
        } else if width > 0.0 {
            wire.world_width = (width * scale) as f32;
        } else {
            wire.set_fixed_screen_width(1.0);
        }
        wires.push(wire);
    }
    if crate::entities::underlay::is_clipped(u) {
        let clip: Vec<[f64; 2]> = crate::entities::underlay::clip_polygon_local(u)
            .into_iter()
            .map(|p| {
                let w = world(p);
                [w[0], w[1]]
            })
            .collect();
        let ring = if u.clip_inverted {
            let [x0, y0, x1, y1] = sheet.rect;
            let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|p| {
                let w = world(p);
                [w[0], w[1]]
            });
            let lo = [corners.iter().map(|p| p[0]).fold(f64::MAX, f64::min), corners.iter().map(|p| p[1]).fold(f64::MAX, f64::min)];
            let hi = [corners.iter().map(|p| p[0]).fold(f64::MIN, f64::max), corners.iter().map(|p| p[1]).fold(f64::MIN, f64::max)];
            crate::scene::pick::xclip::inverted_ring(&clip, (lo, hi))
        } else {
            clip
        };
        crate::scene::pick::xclip::clip_wires(&mut wires, &ring);
    }
    for wire in &mut wires {
        crate::scene::convert::tess::set_wire_aabb(wire, WireModel::UNBOUNDED_AABB);
    }
    wires
}

fn content_wire(name: &str, points: Vec<[f32; 3]>, color: [f32; 4]) -> WireModel {
    let mut wire = WireModel::solid(name.to_string(), points, color, false);
    wire.aci = 0;
    wire.bg_adapt = None;
    wire.display_visible = true;
    wire.plot_visible = true;
    // Drawn, but neither picked nor snapped (see `WireModel::snap_only`).
    wire.snap_only = true;
    wire
}

/// How far a curve's polyline may stray from it: a small share of the
/// sheet, so curves stay smooth when zoomed far in.
fn curve_tolerance(sheet: &Sheet) -> f64 {
    let [x0, y0, x1, y1] = sheet.rect;
    ((x1 - x0).hypot(y1 - y0) * 2e-5).max(1e-9)
}

/// A subpath's points, curves divided within `tolerance`.
fn polyline(segments: &[Segment], tolerance: f64) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = Vec::new();
    for seg in segments {
        let start = seg.start();
        if out.last() != Some(&start) {
            out.push(start);
        }
        match seg {
            Segment::Line(_, b) => out.push(*b),
            Segment::Cubic(a, c1, c2, b) => {
                // The control polygon's length bounds the curve's; the
                // flattening error of n pieces falls as 1/n².
                let hull = dist(*a, *c1) + dist(*c1, *c2) + dist(*c2, *b);
                let n = ((hull / tolerance).sqrt() * 0.5).ceil().clamp(2.0, 256.0) as usize;
                for i in 1..=n {
                    let t = i as f64 / n as f64;
                    let u = 1.0 - t;
                    let (k0, k1, k2, k3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    out.push([
                        k0 * a[0] + k1 * c1[0] + k2 * c2[0] + k3 * b[0],
                        k0 * a[1] + k1 * c1[1] + k2 * c2[1] + k3 * b[1],
                    ]);
                }
            }
        }
    }
    out
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}
