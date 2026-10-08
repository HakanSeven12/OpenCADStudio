//! DWF and DGN underlay content: sheet (DWF) or model (DGN) names, each
//! sheet's geometry in its own units, a display raster of it and the
//! vectors object snaps use — the counterpart of `pdf_raster` and
//! `pdf_vector` for the other underlay kinds.
//!
//! A DWF sheet is in the model units it was plotted from, with the plot's
//! model origin at the underlay's insertion point; a DGN model is in its
//! master units with the global origin there.

mod dgn7;
mod dgn8;
mod dwf;
mod model;
mod wires;

pub(crate) use wires::display_wires;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use codec::entities::UnderlayType;

pub use model::{Path, Segment, Sheet, SubPath};

use super::pdf_raster::{source_bytes, PdfPage};

/// Sheet names (DWF) or model names (DGN) of a file.
pub fn item_names(kind: UnderlayType, path: &str) -> Option<Vec<String>> {
    let bytes = source_bytes(path)?;
    match kind {
        UnderlayType::Dwf => dwf::sheet_names(&bytes),
        UnderlayType::Dgn if dgn7::is_v7(&bytes) => dgn7::model_names(&bytes),
        UnderlayType::Dgn => dgn8::model_names(&bytes),
        UnderlayType::Pdf => None,
    }
}

type Key = (String, String);

fn sheet_cache() -> &'static Mutex<HashMap<Key, Option<Arc<Sheet>>>> {
    static CACHE: OnceLock<Mutex<HashMap<Key, Option<Arc<Sheet>>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The cache key of an item seen without some layers.
fn layered_item(item: &str, hidden: &[String]) -> String {
    if hidden.is_empty() {
        item.to_string()
    } else {
        format!("{item}#{}", hidden.join("|"))
    }
}

fn raster_cache() -> &'static Mutex<HashMap<Key, Option<Arc<PdfPage>>>> {
    static CACHE: OnceLock<Mutex<HashMap<Key, Option<Arc<PdfPage>>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Forget what was read from `path` (its bytes changed).
pub fn forget(path: &str) {
    sheet_cache().lock().unwrap_or_else(|e| e.into_inner()).retain(|k, _| k.0 != path);
    raster_cache().lock().unwrap_or_else(|e| e.into_inner()).retain(|k, _| k.0 != path);
    vector_cache().lock().unwrap_or_else(|e| e.into_inner()).retain(|k, _| k.0 != path);
}

/// Layer (DWF) or level (DGN) names of a file; empty when it has none.
pub fn layer_names(kind: UnderlayType, path: &str) -> Vec<String> {
    let Some(bytes) = source_bytes(path) else { return Vec::new() };
    match kind {
        // A plotted DWF carries no layers.
        UnderlayType::Dwf | UnderlayType::Pdf => None,
        UnderlayType::Dgn if dgn7::is_v7(&bytes) => dgn7::layer_names(&bytes),
        UnderlayType::Dgn => dgn8::layer_names(&bytes),
    }
    .unwrap_or_default()
}

/// The named sheet or model (the first when the name is empty or unknown),
/// memoised. Its text is drawn as strokes with the drawing's text engine.
pub fn sheet(kind: UnderlayType, path: &str, item: &str) -> Option<Arc<Sheet>> {
    sheet_without(kind, path, item, &[])
}

/// The sheet without the `hidden` layers / levels, on the full sheet's
/// rectangle (turning layers off never moves the underlay), memoised.
pub fn sheet_without(kind: UnderlayType, path: &str, item: &str, hidden: &[String]) -> Option<Arc<Sheet>> {
    let key = (path.to_string(), layered_item(item, hidden));
    if let Some(hit) = sheet_cache().lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return hit.clone();
    }
    let read = || -> Option<Sheet> {
        let bytes = source_bytes(path)?;
        let mut sheet = match kind {
            UnderlayType::Dwf => dwf::sheet(&bytes, item)?,
            UnderlayType::Dgn if dgn7::is_v7(&bytes) => dgn7::model(&bytes, hidden).unwrap_or_default(),
            UnderlayType::Dgn => dgn8::model(&bytes, item, hidden).unwrap_or_default(),
            UnderlayType::Pdf => return None,
        };
        // A V7 model's extent includes its text: each text's box runs its
        // length along the baseline and its height above it (as the
        // reference measures a model). A V8 model has its stored element
        // ranges; a DWF sheet keeps its plotted view.
        if kind == UnderlayType::Dgn && dgn7::is_v7(&bytes) {
            let mut r = sheet.rect;
            for t in &sheet.texts {
                let (strokes, _) = crate::scene::text::lff::tessellate_text_ex(
                    [0.0, 0.0],
                    t.height as f32,
                    0.0,
                    t.width_factor as f32,
                    0.0,
                    &t.font,
                    &t.text,
                );
                let length = strokes.iter().flatten().map(|p| p[0] as f64).fold(0.0, f64::max);
                let (c, s) = (t.rotation.cos(), t.rotation.sin());
                for [x, y] in [[0.0, 0.0], [length, 0.0], [length, t.height], [0.0, t.height]] {
                    let p = [t.origin[0] + x * c - y * s, t.origin[1] + x * s + y * c];
                    r = [r[0].min(p[0]), r[1].min(p[1]), r[2].max(p[0]), r[3].max(p[1])];
                }
            }
            sheet.rect = r;
        }
        sheet.element_colors = element_colors(&sheet);
        outline_texts(&mut sheet);
        if !hidden.is_empty() {
            sheet.rect = sheet_without(kind, path, item, &[])?.rect;
        }
        Some(sheet)
    };
    let value = read().map(Arc::new);
    sheet_cache().lock().unwrap_or_else(|e| e.into_inner()).insert(key, value.clone());
    value
}

/// The colour of each path and text of the sheet (a path's fill before
/// its stroke).
fn element_colors(sheet: &Sheet) -> Vec<[u8; 3]> {
    sheet
        .paths
        .iter()
        .filter_map(|p| p.fill.or(p.stroke.map(|s| s.0)))
        .chain(sheet.texts.iter().map(|t| t.color))
        .collect()
}

/// The lightness a DGN model's contrast pulls its colours towards, in
/// thousandths. The reference takes it from the model's own colours as
/// shown (over `backdrop`, faded by `fade` into the background): about
/// 0.788 × their mean lightness, each element counted once (0.788 for an
/// all-white model, 0.416 for mid grey, 0.09 for dark red).
pub fn dgn_contrast_pivot(
    path: &str,
    item: &str,
    hidden: &[String],
    backdrop: Backdrop,
    fade: Option<([u8; 3], u8)>,
) -> Option<u16> {
    let sheet = sheet_without(UnderlayType::Dgn, path, item, hidden)?;
    if sheet.element_colors.is_empty() {
        return None;
    }
    let lightness = |c: [u8; 3]| {
        let c = if sheet.table_colors { model::dgn_table_color(c, backdrop.max_channel, backdrop.light) } else { c };
        let c = match fade {
            Some((bg, f)) => [0, 1, 2].map(|i| c[i] as f64 + (bg[i] as f64 - c[i] as f64) * f.min(100) as f64 / 100.0),
            None => c.map(|v| v as f64),
        };
        (c[0].max(c[1]).max(c[2]) + c[0].min(c[1]).min(c[2])) / 510.0
    };
    let mean = sheet.element_colors.iter().map(|&c| lightness(c)).sum::<f64>() / sheet.element_colors.len() as f64;
    Some((0.788 * mean * 1000.0).round() as u16)
}

/// Text as strokes (and fills for outline fonts), in the text's colour.
fn outline_texts(sheet: &mut Sheet) {
    for t in std::mem::take(&mut sheet.texts) {
        let (strokes, fills) = crate::scene::text::lff::tessellate_text_ex(
            [t.origin[0] as f32, t.origin[1] as f32],
            t.height as f32,
            t.rotation as f32,
            t.width_factor as f32,
            0.0,
            &t.font,
            &t.text,
        );
        let at = |p: [f32; 2]| [p[0] as f64, p[1] as f64];
        let subpaths: Vec<SubPath> = strokes
            .iter()
            .filter(|s| s.len() >= 2)
            .map(|s| SubPath { segments: s.windows(2).map(|w| Segment::Line(at(w[0]), at(w[1]))).collect(), closed: false })
            .collect();
        if !subpaths.is_empty() {
            sheet.paths.push(Path { subpaths, stroke: Some((t.color, -1.0)), fill: None });
        }
        let triangles: Vec<SubPath> = fills
            .chunks_exact(3)
            .map(|t| SubPath {
                segments: vec![Segment::Line(at(t[0]), at(t[1])), Segment::Line(at(t[1]), at(t[2])), Segment::Line(at(t[2]), at(t[0]))],
                closed: true,
            })
            .collect();
        if !triangles.is_empty() {
            sheet.paths.push(Path { subpaths: triangles, stroke: None, fill: Some(t.color) });
        }
    }
}

/// Longest side of a sheet's full-size display raster, pixels.
pub const RASTER_SIDE: f64 = 3072.0;

/// The background a sheet is shown over, as far as its colours depend on
/// it: the largest channel (in the steps a DGN colour table changes at) and
/// whether the background is light.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Backdrop {
    pub max_channel: u8,
    pub light: bool,
}

impl Backdrop {
    pub fn of(rgb: [f32; 3]) -> Self {
        let max = rgb[0].max(rgb[1]).max(rgb[2]);
        let min = rgb[0].min(rgb[1]).min(rgb[2]);
        let top = (max.clamp(0.0, 1.0) * 255.0).round() as u8;
        Backdrop {
            max_channel: match top {
                0..=29 => 0,
                30..=31 => 30,
                _ => 32,
            },
            // Light means an HSL lightness of one half or more.
            light: (max + min) / 2.0 >= 0.5,
        }
    }
}

/// The sheet drawn on a transparent background with `side` pixels on its
/// longest side (at most [`RASTER_SIDE`]), straight alpha, rows from the
/// top, memoised per size and backdrop.
pub fn display_raster(
    kind: UnderlayType,
    path: &str,
    item: &str,
    hidden: &[String],
    side: f64,
    backdrop: Backdrop,
) -> Option<Arc<PdfPage>> {
    let side = side.clamp(16.0, RASTER_SIDE).round();
    let key = (
        path.to_string(),
        format!("{}@{side}~{}{}", layered_item(item, hidden), backdrop.max_channel, backdrop.light),
    );
    if let Some(hit) = raster_cache().lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return hit.clone();
    }
    let value = sheet_without(kind, path, item, hidden).and_then(|s| rasterize(&s, side, backdrop)).map(Arc::new);
    raster_cache().lock().unwrap_or_else(|e| e.into_inner()).insert(key, value.clone());
    value
}

fn rasterize(sheet: &Sheet, side: f64, backdrop: Backdrop) -> Option<PdfPage> {
    let shown = |c: [u8; 3]| {
        if sheet.table_colors {
            model::dgn_table_color(c, backdrop.max_channel, backdrop.light)
        } else {
            c
        }
    };
    let [x0, y0, x1, y1] = sheet.rect;
    let (w, h) = (x1 - x0, y1 - y0);
    if !(w > 0.0 && h > 0.0) {
        return None;
    }
    let scale = side / w.max(h);
    let (pw, ph) = ((w * scale).ceil().max(1.0) as u32, (h * scale).ceil().max(1.0) as u32);
    let mut pixmap = tiny_skia::Pixmap::new(pw, ph)?;
    // The rectangle's edges fall on the centres of the outer pixels, so a
    // line along the edge of the extent (every extent has some) is drawn.
    let (sx, sy) = ((pw.max(2) - 1) as f64 / w, (ph.max(2) - 1) as f64 / h);
    let to_px = |p: [f64; 2]| (((p[0] - x0) * sx + 0.5) as f32, ((y1 - p[1]) * sy + 0.5) as f32);
    for path in &sheet.paths {
        let mut pb = tiny_skia::PathBuilder::new();
        for sp in &path.subpaths {
            let Some(first) = sp.segments.first() else { continue };
            let s = to_px(first.start());
            pb.move_to(s.0, s.1);
            for seg in &sp.segments {
                match seg {
                    Segment::Line(_, b) => {
                        let b = to_px(*b);
                        pb.line_to(b.0, b.1);
                    }
                    Segment::Cubic(_, c1, c2, b) => {
                        let (c1, c2, b) = (to_px(*c1), to_px(*c2), to_px(*b));
                        pb.cubic_to(c1.0, c1.1, c2.0, c2.1, b.0, b.1);
                    }
                }
            }
            if sp.closed {
                pb.close();
            }
        }
        let Some(p) = pb.finish() else { continue };
        if let Some(c) = path.fill.map(shown) {
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(c[0], c[1], c[2], 255);
            // Fills that share an edge (triangle strips) would show a seam.
            paint.anti_alias = false;
            pixmap.fill_path(&p, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
        }
        if let Some((c, width)) = path.stroke.map(|(c, w)| (shown(c), w)) {
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(c[0], c[1], c[2], 255);
            // A negative width is in pixels (DGN line weights).
            let px = if width < 0.0 { (-width) as f32 } else { ((width * scale) as f32).max(1.0) };
            let stroke = tiny_skia::Stroke {
                width: px,
                line_cap: tiny_skia::LineCap::Round,
                line_join: tiny_skia::LineJoin::Round,
                ..Default::default()
            };
            if px > 1.5 {
                paint.anti_alias = true;
                pixmap.stroke_path(&p, &paint, &stroke, tiny_skia::Transform::identity(), None);
            } else {
                // Hairlines: horizontal and vertical pieces solid, one pixel
                // wide, as the reference draws them (anti-aliasing would
                // spread them to half intensity); slanted pieces and curves
                // anti-aliased, since their pixel steps break up once the
                // raster is scaled to the screen.
                let (straight, curved) = hairline_parts(path, &to_px);
                paint.anti_alias = false;
                if let Some(p) = straight {
                    pixmap.stroke_path(&p, &paint, &stroke, tiny_skia::Transform::identity(), None);
                }
                paint.anti_alias = true;
                if let Some(p) = curved {
                    pixmap.stroke_path(&p, &paint, &stroke, tiny_skia::Transform::identity(), None);
                }
            }
        }
    }
    let mut pixels = pixmap.take();
    for px in pixels.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Some(PdfPage { pixels: Arc::new(pixels), width: pw, height: ph, dpi: 0.0 })
}

/// A path's horizontal and vertical segments, and its slanted and curved
/// ones, each as a pixel path.
fn hairline_parts(
    path: &Path,
    to_px: &dyn Fn([f64; 2]) -> (f32, f32),
) -> (Option<tiny_skia::Path>, Option<tiny_skia::Path>) {
    let (mut straight, mut curved) = (tiny_skia::PathBuilder::new(), tiny_skia::PathBuilder::new());
    // Consecutive segments of one kind stay one run, so their joins are
    // drawn once.
    let (mut straight_at, mut curved_at) = (None, None);
    for sp in &path.subpaths {
        for seg in &sp.segments {
            let a = to_px(seg.start());
            match seg {
                Segment::Line(_, b) if along_axis(a, to_px(*b)) => {
                    let b = to_px(*b);
                    if straight_at != Some(a) {
                        straight.move_to(a.0, a.1);
                    }
                    straight.line_to(b.0, b.1);
                    straight_at = Some(b);
                }
                Segment::Line(_, b) => {
                    let b = to_px(*b);
                    if curved_at != Some(a) {
                        curved.move_to(a.0, a.1);
                    }
                    curved.line_to(b.0, b.1);
                    curved_at = Some(b);
                }
                Segment::Cubic(_, c1, c2, b) => {
                    let (c1, c2, b) = (to_px(*c1), to_px(*c2), to_px(*b));
                    if curved_at != Some(a) {
                        curved.move_to(a.0, a.1);
                    }
                    curved.cubic_to(c1.0, c1.1, c2.0, c2.1, b.0, b.1);
                    curved_at = Some(b);
                }
            }
        }
        straight_at = None;
        curved_at = None;
    }
    (straight.finish(), curved.finish())
}

/// A pixel segment that runs horizontally or vertically.
fn along_axis(a: (f32, f32), b: (f32, f32)) -> bool {
    (b.0 - a.0).abs() < 0.01 || (b.1 - a.1).abs() < 0.01
}

fn vector_cache() -> &'static Mutex<HashMap<Key, Option<Arc<super::pdf_vector::PageVectors>>>> {
    static CACHE: OnceLock<Mutex<HashMap<Key, Option<Arc<super::pdf_vector::PageVectors>>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The sheet's geometry as page vectors for object snaps, in sheet units,
/// memoised.
pub fn page_vectors(kind: UnderlayType, path: &str, item: &str, hidden: &[String]) -> Option<Arc<super::pdf_vector::PageVectors>> {
    let key = (path.to_string(), layered_item(item, hidden));
    if let Some(hit) = vector_cache().lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return hit.clone();
    }
    let value = sheet_without(kind, path, item, hidden).map(|s| Arc::new(to_page_vectors(&s)));
    vector_cache().lock().unwrap_or_else(|e| e.into_inner()).insert(key, value.clone());
    value
}

fn to_page_vectors(sheet: &Sheet) -> super::pdf_vector::PageVectors {
    use super::pdf_vector as pv;
    let paths = sheet
        .paths
        .iter()
        .map(|p| pv::PdfPath {
            subpaths: p
                .subpaths
                .iter()
                .map(|sp| pv::SubPath {
                    segments: sp
                        .segments
                        .iter()
                        .map(|s| match s {
                            Segment::Line(a, b) => pv::Segment::Line(*a, *b),
                            Segment::Cubic(a, b, c, d) => pv::Segment::Cubic(*a, *b, *c, *d),
                        })
                        .collect(),
                    closed: sp.closed,
                })
                .collect(),
            stroke: p.stroke.map(|(c, w)| (c, w.max(0.0))),
            fill: p.fill,
        })
        .collect();
    pv::PageVectors { paths, ..Default::default() }
}
