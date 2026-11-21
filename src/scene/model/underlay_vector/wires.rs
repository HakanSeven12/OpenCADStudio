//! A DWF sheet or DGN model drawn as vector wires (lines and flat fills)
//! in world space, so it stays sharp at every zoom as the reference draws
//! it, instead of as a raster rescaled to the view.

use std::collections::HashMap;

use document::entities::{Underlay, UnderlayDefinition, UnderlayDisplayFlags, UnderlayType};

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
    if def.underlay_type == UnderlayType::Pdf
        || def.unloaded
        || !u.flags.contains(UnderlayDisplayFlags::ON)
    {
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
    let alpha = if def.underlay_type == UnderlayType::Dgn {
        1.0
    } else {
        1.0 - u.fade.min(100) as f32 / 100.0
    };
    let shown = |c: [u8; 3]| -> [f32; 4] {
        let c = if sheet.table_colors {
            model::dgn_table_color(c, backdrop.max_channel, backdrop.light)
        } else {
            c
        };
        let rgb = crate::scene::model::pdf_raster::adjust_rgb(
            [
                c[0] as f32 / 255.0,
                c[1] as f32 / 255.0,
                c[2] as f32 / 255.0,
            ],
            &adjust,
        );
        [rgb[0], rgb[1], rgb[2], alpha]
    };
    let world = |p: [f64; 2]| crate::entities::underlay::local_to_world(u, p);
    let scale = u.x_scale.abs().max(u.y_scale.abs());
    let mut wires = content_wires(&sheet, name, &shown, &world, scale);
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
            let lo = [
                corners.iter().map(|p| p[0]).fold(f64::MAX, f64::min),
                corners.iter().map(|p| p[1]).fold(f64::MAX, f64::min),
            ];
            let hi = [
                corners.iter().map(|p| p[0]).fold(f64::MIN, f64::max),
                corners.iter().map(|p| p[1]).fold(f64::MIN, f64::max),
            ];
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

/// The sheet's fills and lines as wires named `name`: `shown` gives a colour
/// of the sheet as it is drawn, `world` places a point of the sheet and
/// `scale` is what a width of the sheet measures there.
fn content_wires(
    sheet: &Sheet,
    name: &str,
    shown: &dyn Fn([u8; 3]) -> [f32; 4],
    world: &dyn Fn([f64; 2]) -> [f64; 3],
    scale: f64,
) -> Vec<WireModel> {
    let tolerance = curve_tolerance(sheet);

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
                let (points, triangles) = ocs_geom::geom2d::triangulate_rings(&rings);
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
            let at = *line_at
                .entry((key(color), width.to_bits()))
                .or_insert_with(|| {
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
        wire.points_low = lp_low.into();
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

#[cfg(test)]
mod draw_order_tests {
    use super::super::model::{Path, SubPath};
    use super::*;

    const RED: [u8; 3] = [220, 0, 0];
    const YELLOW: [u8; 3] = [255, 220, 0];
    const BLUE: [u8; 3] = [0, 90, 255];

    /// A filled box from `low` to `high`.
    fn fill(color: [u8; 3], low: [f64; 2], high: [f64; 2]) -> Path {
        let corners = [low, [high[0], low[1]], high, [low[0], high[1]]];
        Path {
            subpaths: vec![SubPath {
                segments: (0..4)
                    .map(|k| Segment::Line(corners[k], corners[(k + 1) % 4]))
                    .collect(),
                closed: true,
            }],
            stroke: None,
            fill: Some(color),
        }
    }

    /// A line from `a` to `b`, one pixel wide.
    fn line(color: [u8; 3], a: [f64; 2], b: [f64; 2]) -> Path {
        Path {
            subpaths: vec![SubPath {
                segments: vec![Segment::Line(a, b)],
                closed: false,
            }],
            stroke: Some((color, 0.0)),
            fill: None,
        }
    }

    /// The wires of a sheet 300 by 200 that draws `paths` in that order.
    fn wires_of(paths: Vec<Path>) -> Vec<WireModel> {
        let sheet = Sheet {
            rect: [0.0, 0.0, 300.0, 200.0],
            paths,
            ..Default::default()
        };
        let shown = |c: [u8; 3]| {
            [
                c[0] as f32 / 255.0,
                c[1] as f32 / 255.0,
                c[2] as f32 / 255.0,
                1.0,
            ]
        };
        let world = |p: [f64; 2]| [p[0], p[1], 0.0];
        content_wires(&sheet, "1A", &shown, &world, 1.0)
    }

    /// Whether a wire is of `color`.
    fn is(wire: &WireModel, color: [u8; 3]) -> bool {
        (0..3).all(|k| (wire.color[k] - color[k] as f32 / 255.0).abs() < 1e-6)
    }

    /// The wire that draws something at `at`: a corner of one of its fill
    /// triangles or a point of one of its lines.
    fn wire_at(wires: &[WireModel], at: [f64; 2]) -> &WireModel {
        let on =
            |p: &[f32; 3]| (p[0] as f64 - at[0]).abs() < 1e-3 && (p[1] as f64 - at[1]).abs() < 1e-3;
        wires
            .iter()
            .find(|wire| wire.fill_tris.iter().any(on) || wire.points.iter().any(on))
            .unwrap_or_else(|| panic!("nothing is drawn at {at:?}"))
    }

    /// Where a wire stands in the underlay's own draw order.
    fn rank(wire: &WireModel) -> f32 {
        wire.depth_override.unwrap_or(0.0)
    }

    #[test]
    fn a_fill_drawn_over_one_of_another_colour_stays_over_it() {
        // Red is in use before the yellow box is drawn; the red block is
        // drawn after the box, on it.
        let wires = wires_of(vec![
            fill(RED, [20.0, 150.0], [50.0, 180.0]),
            fill(YELLOW, [100.0, 80.0], [200.0, 120.0]),
            fill(RED, [120.0, 92.0], [180.0, 108.0]),
        ]);

        let square = wire_at(&wires, [20.0, 150.0]);
        let boxed = wire_at(&wires, [100.0, 80.0]);
        let block = wire_at(&wires, [120.0, 92.0]);
        assert!(is(block, RED) && is(boxed, YELLOW));
        assert!(
            rank(block) > rank(boxed),
            "the block at {} is not over the box at {}",
            rank(block),
            rank(boxed)
        );
        assert!(rank(square) <= rank(boxed));
    }

    #[test]
    fn a_fill_drawn_after_lines_lies_over_them_and_under_later_ones() {
        let wires = wires_of(vec![
            line(BLUE, [60.0, 60.0], [100.0, 140.0]),
            fill(YELLOW, [70.0, 80.0], [200.0, 120.0]),
            line(BLUE, [80.0, 86.0], [190.0, 114.0]),
        ]);

        let under = wire_at(&wires, [60.0, 60.0]);
        let boxed = wire_at(&wires, [70.0, 80.0]);
        let over = wire_at(&wires, [80.0, 86.0]);
        assert!(
            rank(under) < rank(boxed) && rank(boxed) < rank(over),
            "{} {} {}",
            rank(under),
            rank(boxed),
            rank(over)
        );
    }

    #[test]
    fn what_lies_apart_shares_a_wire_with_its_like() {
        // The second red box is nowhere near the yellow one drawn before it.
        let wires = wires_of(vec![
            fill(RED, [20.0, 150.0], [50.0, 180.0]),
            fill(YELLOW, [100.0, 80.0], [200.0, 120.0]),
            fill(RED, [240.0, 20.0], [280.0, 40.0]),
            line(BLUE, [10.0, 10.0], [60.0, 10.0]),
            line(BLUE, [10.0, 190.0], [60.0, 190.0]),
        ]);

        assert_eq!(wires.len(), 3, "red fills, the yellow one and blue lines");
        assert!(std::ptr::eq(
            wire_at(&wires, [20.0, 150.0]),
            wire_at(&wires, [240.0, 20.0])
        ));
        assert!(std::ptr::eq(
            wire_at(&wires, [10.0, 10.0]),
            wire_at(&wires, [10.0, 190.0])
        ));
    }

    #[test]
    fn a_sheet_of_one_colour_and_kind_is_one_wire_at_the_underlays_depth() {
        let wires = wires_of(vec![
            fill(RED, [20.0, 150.0], [50.0, 180.0]),
            fill(RED, [30.0, 160.0], [90.0, 170.0]),
        ]);

        assert_eq!(wires.len(), 1);
        assert_eq!(wires[0].depth_override, None);
    }
}
