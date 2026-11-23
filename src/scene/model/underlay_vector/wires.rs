//! A DWF sheet or DGN model drawn as vector wires (lines and flat fills)
//! in world space, so it stays sharp at every zoom as the reference draws
//! it, instead of as a raster rescaled to the view.

use std::collections::HashMap;

use document::entities::{Underlay, UnderlayDefinition, UnderlayDisplayFlags, UnderlayType};

use super::{model, sheet_without, Backdrop, Segment, Sheet};
use crate::scene::model::wire_model::WireModel;

/// Wires for the underlay's content, named after the underlay so they draw
/// in its place, in the order the sheet draws them ([`content_wires`]). They
/// are drawn and plotted but never picked or snapped (the frame selects the
/// underlay; the underlay's own snap geometry serves object snaps).
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

/// A side of the grid a sheet is cut into to tell where its content lies.
const GRID: usize = 64;

/// The most wires a sheet's content is drawn with. Past it a piece joins the
/// latest wire of its kind, whatever was drawn since.
const MAX_BATCHES: usize = 256;

/// The cells of a [`GRID`] by `GRID` grid over the sheet that some content
/// touches.
#[derive(Clone)]
struct Cover([u64; GRID * GRID / 64]);

impl Cover {
    fn new() -> Self {
        Self([0; GRID * GRID / 64])
    }

    /// The column and row of the cell a point of the sheet falls in; a
    /// point off the sheet counts for the nearest cell.
    fn cell(rect: [f64; 4], p: [f64; 2]) -> [usize; 2] {
        let along = |v: f64, low: f64, high: f64| {
            let share = (v - low) / (high - low).max(f64::MIN_POSITIVE);
            ((share * GRID as f64).max(0.0) as usize).min(GRID - 1)
        };
        [along(p[0], rect[0], rect[2]), along(p[1], rect[1], rect[3])]
    }

    /// Marks every cell of the box from `low` to `high`.
    fn mark_box(&mut self, rect: [f64; 4], low: [f64; 2], high: [f64; 2]) {
        let ([c0, r0], [c1, r1]) = (Self::cell(rect, low), Self::cell(rect, high));
        for row in r0..=r1 {
            for col in c0..=c1 {
                self.0[(row * GRID + col) / 64] |= 1 << ((row * GRID + col) % 64);
            }
        }
    }

    /// Marks the cells the segment from `a` to `b` runs through.
    fn mark_segment(&mut self, rect: [f64; 4], a: [f64; 2], b: [f64; 2]) {
        let ([c0, r0], [c1, r1]) = (Self::cell(rect, a), Self::cell(rect, b));
        // Two samples to the cell along the longer side keep the run whole.
        let steps = 2 * c0.abs_diff(c1).max(r0.abs_diff(r1)) + 1;
        for step in 0..=steps {
            let s = step as f64 / steps as f64;
            let at = [a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s];
            self.mark_box(rect, at, at);
        }
    }

    fn meets(&self, other: &Self) -> bool {
        self.0.iter().zip(&other.0).any(|(a, b)| a & b != 0)
    }

    fn add(&mut self, other: &Self) {
        for (mine, theirs) in self.0.iter_mut().zip(&other.0) {
            *mine |= theirs;
        }
    }
}

/// Content drawn in one go: fills of one colour, or lines of one colour and
/// width.
struct Batch {
    color: [f32; 4],
    /// The lines' width; `None` for fills.
    width: Option<f64>,
    /// The corners of the fill triangles, or the lines' points with a NaN
    /// between two runs.
    points: Vec<[f64; 3]>,
    cover: Cover,
}

/// The sheet's fills and lines as wires named `name`: `shown` gives a colour
/// of the sheet as it is drawn, `world` places a point of the sheet and
/// `scale` is what a width of the sheet measures there.
///
/// A sheet is drawn in order, each fill and line over what came before it.
/// The wires keep that order: a piece joins the latest wire of its kind and
/// colour unless something drawn since lies where the piece does, and then
/// it starts a wire of its own. Each wire takes its place in the underlay's
/// own depth range, as a block's members take theirs, so a fill covers the
/// lines drawn before it and carries what is drawn after it.
///
/// The sheet's raster images are one picture, drawn at the underlay's own
/// depth (`ImageModel::from_underlay`), and they take their place in the
/// order as one: from the first image on, what lies on an image is drawn
/// over the picture, and the rest stays under it with what came before.
fn content_wires(
    sheet: &Sheet,
    name: &str,
    shown: &dyn Fn([u8; 3]) -> [f32; 4],
    world: &dyn Fn([f64; 2]) -> [f64; 3],
    scale: f64,
) -> Vec<WireModel> {
    let tolerance = curve_tolerance(sheet);
    type Kind = ([u32; 4], Option<u64>);
    let mut batches: Vec<Batch> = Vec::new();
    let mut latest: HashMap<Kind, usize> = HashMap::new();
    // The sheet's picture: how many batches lie under it and where its
    // images lie.
    let mut picture: Option<(usize, Cover)> = None;
    // The batch a piece of this colour and width that touches `cover` goes to.
    let mut place = |batches: &mut Vec<Batch>,
                     picture: &Option<(usize, Cover)>,
                     color: [f32; 4],
                     width: Option<f64>,
                     cover: Cover| {
        let kind: Kind = (color.map(f32::to_bits), width.map(f64::to_bits));
        if let Some(&at) = latest.get(&kind) {
            let clear = batches[at + 1..]
                .iter()
                .all(|later| !later.cover.meets(&cover))
                && !picture
                    .as_ref()
                    .is_some_and(|(under, images)| *under > at && images.meets(&cover));
            if clear || batches.len() >= MAX_BATCHES {
                batches[at].cover.add(&cover);
                return at;
            }
        }
        batches.push(Batch {
            color,
            width,
            points: Vec::new(),
            cover,
        });
        latest.insert(kind, batches.len() - 1);
        batches.len() - 1
    };
    // An image joins the picture, which lies over the batches there are
    // when its first image comes.
    let join = |picture: &mut Option<(usize, Cover)>, under: usize, image: &model::SheetImage| {
        let [x0, y0, x1, y1] = image.rect;
        let (_, images) = picture.get_or_insert_with(|| (under, Cover::new()));
        images.mark_box(sheet.rect, [x0, y0], [x1, y1]);
    };
    let mut images = sheet.images.iter().peekable();
    for (index, path) in sheet.paths.iter().enumerate() {
        while let Some(image) = images.next_if(|image| image.after <= index) {
            join(&mut picture, batches.len(), image);
        }
        if let Some(c) = path.fill {
            let rings: Vec<Vec<[f64; 2]>> = path
                .subpaths
                .iter()
                .map(|sp| polyline(&sp.segments, tolerance))
                .filter(|r| r.len() >= 3)
                .collect();
            if !rings.is_empty() {
                let mut cover = Cover::new();
                for ring in &rings {
                    let reach = |axis: usize, most: bool| {
                        let values = ring.iter().map(|p| p[axis]);
                        if most {
                            values.fold(f64::NEG_INFINITY, f64::max)
                        } else {
                            values.fold(f64::INFINITY, f64::min)
                        }
                    };
                    cover.mark_box(
                        sheet.rect,
                        [reach(0, false), reach(1, false)],
                        [reach(0, true), reach(1, true)],
                    );
                }
                let at = place(&mut batches, &picture, shown(c), None, cover);
                let (points, triangles) = ocs_geom::geom2d::triangulate_rings(&rings);
                for triangle in triangles {
                    for index in triangle {
                        if let Some(&p) = points.get(index) {
                            batches[at].points.push(world(p));
                        }
                    }
                }
            }
        }
        if let Some((c, width)) = path.stroke {
            let runs: Vec<Vec<[f64; 2]>> = path
                .subpaths
                .iter()
                .filter_map(|sp| {
                    let mut pts = polyline(&sp.segments, tolerance);
                    if sp.closed && pts.len() >= 2 && pts.first() != pts.last() {
                        pts.push(pts[0]);
                    }
                    (pts.len() >= 2).then_some(pts)
                })
                .collect();
            if !runs.is_empty() {
                let mut cover = Cover::new();
                for run in &runs {
                    for pair in run.windows(2) {
                        cover.mark_segment(sheet.rect, pair[0], pair[1]);
                    }
                }
                let at = place(&mut batches, &picture, shown(c), Some(width), cover);
                for run in runs {
                    let points = &mut batches[at].points;
                    if !points.is_empty() {
                        points.push([f64::NAN; 3]);
                    }
                    points.extend(run.into_iter().map(world));
                }
            }
        }
    }

    for image in images {
        join(&mut picture, batches.len(), image);
    }

    let count = batches.len();
    let under = picture.map(|(under, _)| under);
    batches
        .into_iter()
        .enumerate()
        .map(|(index, batch)| {
            let (high, low) = crate::scene::convert::tessellate::points_to_ds(batch.points);
            let mut wire = match batch.width {
                None => {
                    let mut wire = content_wire(name, Vec::new(), batch.color);
                    wire.fill_tris = high;
                    wire.fill_tris_low = low;
                    wire
                }
                Some(width) => {
                    let mut wire = content_wire(name, high, batch.color);
                    wire.points_low = low.into();
                    if width < 0.0 {
                        // A width in pixels (DGN line weights are not shown).
                        wire.set_fixed_screen_width((-width) as f32);
                    } else if width > 0.0 {
                        wire.world_width = (width * scale) as f32;
                    } else {
                        wire.set_fixed_screen_width(1.0);
                    }
                    wire
                }
            };
            // The first wire lowest and the last highest in the underlay's
            // depth range; a lone wire stays at the underlay's own depth.
            // The sheet's picture is drawn at that depth, so with one the
            // wires under it take the lower half and the others the upper.
            wire.depth_override = match under {
                Some(under) if index < under => Some(-0.9 * (under - index) as f32 / under as f32),
                Some(under) => Some(0.9 * (index + 1 - under) as f32 / (count - under) as f32),
                None if count > 1 => Some(-0.9 + 1.8 * index as f32 / (count - 1) as f32),
                None => None,
            };
            wire
        })
        .collect()
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
    use super::super::model::{Path, SheetImage, SubPath};
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
        wires_with(paths, Vec::new())
    }

    /// An image over the box from `low` to `high`, drawn after the first
    /// `after` paths of its sheet.
    fn picture(low: [f64; 2], high: [f64; 2], after: usize) -> SheetImage {
        SheetImage {
            rect: [low[0], low[1], high[0], high[1]],
            width: 1,
            height: 1,
            pixels: std::sync::Arc::new(vec![255; 4]),
            after,
        }
    }

    /// The wires of a sheet 300 by 200 that draws `paths` in that order and
    /// `images` among them.
    fn wires_with(paths: Vec<Path>, images: Vec<SheetImage>) -> Vec<WireModel> {
        let sheet = Sheet {
            rect: [0.0, 0.0, 300.0, 200.0],
            paths,
            images,
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

    // The images of a sheet are drawn as one picture at the underlay's own
    // depth, rank 0: the wires take the ranks under it and over it.

    #[test]
    fn what_is_drawn_before_an_image_ranks_under_it_and_what_comes_after_over_it() {
        let wires = wires_with(
            vec![
                fill(YELLOW, [0.0, 0.0], [300.0, 200.0]),
                line(BLUE, [10.0, 10.0], [290.0, 190.0]),
                line(RED, [60.0, 60.0], [100.0, 140.0]),
            ],
            vec![picture([50.0, 50.0], [250.0, 150.0], 2)],
        );

        let paper = wire_at(&wires, [0.0, 0.0]);
        let under = wire_at(&wires, [10.0, 10.0]);
        let over = wire_at(&wires, [60.0, 60.0]);
        assert!(
            rank(paper) < rank(under) && rank(under) < 0.0 && 0.0 < rank(over),
            "{} {} {}",
            rank(paper),
            rank(under),
            rank(over)
        );
    }

    #[test]
    fn a_lone_wire_takes_its_side_of_an_image() {
        let over = wires_with(
            vec![line(BLUE, [60.0, 60.0], [100.0, 140.0])],
            vec![picture([50.0, 50.0], [250.0, 150.0], 0)],
        );
        let under = wires_with(
            vec![line(BLUE, [60.0, 60.0], [100.0, 140.0])],
            vec![picture([50.0, 50.0], [250.0, 150.0], 1)],
        );

        assert_eq!((over.len(), under.len()), (1, 1));
        assert!(over[0].depth_override.is_some_and(|rank| rank > 0.0));
        assert!(under[0].depth_override.is_some_and(|rank| rank < 0.0));
    }

    #[test]
    fn what_is_drawn_after_an_image_away_from_it_joins_its_like_under_it() {
        let wires = wires_with(
            vec![
                line(BLUE, [10.0, 10.0], [40.0, 10.0]),
                line(BLUE, [10.0, 190.0], [40.0, 190.0]),
            ],
            vec![picture([100.0, 50.0], [250.0, 150.0], 1)],
        );

        assert_eq!(wires.len(), 1);
        assert!(rank(&wires[0]) < 0.0);
    }

    #[test]
    fn a_second_image_is_drawn_with_the_first() {
        // A fill where the second image will lie, the first image, a line on
        // it, the second image, a line on that one.
        let wires = wires_with(
            vec![
                fill(YELLOW, [160.0, 20.0], [280.0, 180.0]),
                line(RED, [30.0, 30.0], [110.0, 170.0]),
                line(BLUE, [170.0, 30.0], [270.0, 170.0]),
            ],
            vec![
                picture([20.0, 20.0], [120.0, 180.0], 1),
                picture([160.0, 20.0], [280.0, 180.0], 2),
            ],
        );

        assert!(
            rank(wire_at(&wires, [160.0, 20.0])) < 0.0,
            "the fill is under"
        );
        assert!(rank(wire_at(&wires, [30.0, 30.0])) > 0.0);
        assert!(rank(wire_at(&wires, [170.0, 30.0])) > 0.0);
    }
}
