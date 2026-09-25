//! Vector content of a PDF page — paths and text runs in page inches (y up,
//! origin at the page's lower-left corner) — read through the PDF
//! interpreter. Drives snapping to an underlay's geometry and PDFIMPORT.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use hayro::hayro_interpret::font::Glyph;
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_interpret::{
    interpret_page, BlendMode, ClipPath, Context, Device, GlyphDrawMode, Image, InterpreterCache,
    InterpreterSettings, Paint, PathDrawMode, SoftMask,
};
use hayro::hayro_syntax::content::ops::TypedInstruction;
use hayro::hayro_syntax::object::Name;
use hayro::hayro_syntax::Pdf;
use kurbo::{Affine, BezPath, PathEl, Point, Rect, Shape};

/// One segment of a path, in page inches.
#[derive(Clone, Debug, PartialEq)]
pub enum Segment {
    Line([f64; 2], [f64; 2]),
    Cubic([f64; 2], [f64; 2], [f64; 2], [f64; 2]),
}

impl Segment {
    pub fn start(&self) -> [f64; 2] {
        match self {
            Segment::Line(a, _) | Segment::Cubic(a, ..) => *a,
        }
    }
    pub fn end(&self) -> [f64; 2] {
        match self {
            Segment::Line(_, b) | Segment::Cubic(_, _, _, b) => *b,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SubPath {
    pub segments: Vec<Segment>,
    pub closed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PdfPath {
    pub subpaths: Vec<SubPath>,
    /// Stroke colour (RGB) and width in points, for stroked paths.
    pub stroke: Option<([u8; 3], f64)>,
    /// Fill colour (RGB), for filled paths.
    pub fill: Option<[u8; 3]>,
}

/// A run of glyphs on one baseline in one font and size.
#[derive(Clone, Debug, PartialEq)]
pub struct PdfText {
    pub text: String,
    /// Base font name without a subset prefix ("Helvetica").
    pub font: String,
    /// Baseline start, page inches.
    pub origin: [f64; 2],
    /// Height of the capital letters, inches.
    pub cap_height: f64,
    /// Advance width of the run, inches.
    pub width: f64,
    /// Baseline direction, radians.
    pub rotation: f64,
    pub color: [u8; 3],
}

/// A raster image drawn on a page: its pixels and where they land.
#[derive(Clone, Debug, PartialEq)]
pub struct PdfImage {
    /// Straight RGBA, rows from the top.
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Lower-left corner, page inches.
    pub origin: [f64; 2],
    /// One pixel along the image's rows and up its columns, inches.
    pub u: [f64; 2],
    pub v: [f64; 2],
}

#[derive(Default, Debug)]
pub struct PageVectors {
    pub paths: Vec<PdfPath>,
    pub texts: Vec<PdfText>,
    /// Filled only when images are asked for (`page_content`).
    pub images: Vec<PdfImage>,
    /// Paths, texts and images in the order the page draws them.
    pub order: Vec<Drawn>,
}

/// One drawn item: an index into `paths`, `texts` or `images`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drawn {
    Path(usize),
    Text(usize),
    Image(usize),
}

type Key = (String, String);

fn cache() -> &'static Mutex<HashMap<Key, Option<Arc<PageVectors>>>> {
    static CACHE: OnceLock<Mutex<HashMap<Key, Option<Arc<PageVectors>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Paths and text of a 1-based page, memoised per source and page.
pub fn page_vectors(path: &str, page: &str) -> Option<Arc<PageVectors>> {
    let key = (path.to_string(), page.to_string());
    if let Some(hit) = cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&key)
        .cloned()
    {
        return hit;
    }
    let built = read_page(path, page).map(Arc::new);
    cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(key, built.clone());
    built
}

/// A page's paths, text and raster images (not cached; import only).
pub fn page_content(path: &str, page: &str) -> Option<PageVectors> {
    read_page_with(path, page, true)
}

/// A page's content split by PDF layer: `None` for what no layer holds,
/// then each named layer that is not in `hidden`. Each layer's share is
/// what the page shows with that layer alone on, less what it shows with
/// every layer off.
// ponytail: content in several layers at once (membership dictionaries) is
// credited to the first layer that shows it.
pub fn page_content_by_layer(
    path: &str,
    page: &str,
    hidden: &[String],
) -> Option<Vec<(Option<String>, PageVectors)>> {
    let layers = super::pdf_layers::layers(path);
    if layers.is_empty() {
        return Some(vec![(None, page_content(path, page)?)]);
    }
    let names: Vec<String> = layers.iter().map(|l| l.name.clone()).collect();
    let base = page_content(&super::pdf_layers::source_with_layers(path, &names), page)?;
    let key = |s: &dyn std::fmt::Debug| format!("{s:?}");
    let mut seen: HashMap<String, usize> = HashMap::new();
    for item in base.paths.iter().map(|p| key(p)).chain(base.texts.iter().map(|t| key(t))) {
        *seen.entry(item).or_default() += 1;
    }
    let mut out = Vec::new();
    for layer in &layers {
        if hidden.contains(&layer.name) {
            continue;
        }
        let others: Vec<String> = names.iter().filter(|n| **n != layer.name).cloned().collect();
        let Some(mut only) =
            page_content(&super::pdf_layers::source_with_layers(path, &others), page)
        else {
            continue;
        };
        let mut left = seen.clone();
        let mut fresh = |item: String| match left.get_mut(&item) {
            Some(n) if *n > 0 => {
                *n -= 1;
                false
            }
            _ => true,
        };
        only.paths.retain(|p| fresh(key(p)));
        only.texts.retain(|t| fresh(key(t)));
        only.images.retain(|i| !base.images.contains(i));
        out.push((Some(layer.name.clone()), only));
    }
    let mut base = base;
    images_to_last_layer(path, page, &mut base, &mut out);
    out.insert(0, (None, base));
    Some(out)
}

/// An image that no layer holds goes with the layer drawn last before it
/// (with none before it, it stays unlayered): the page is read with every
/// layer on for the drawing order.
fn images_to_last_layer(
    path: &str,
    page: &str,
    base: &mut PageVectors,
    layered: &mut [(Option<String>, PageVectors)],
) {
    if base.images.is_empty() || layered.is_empty() {
        return;
    }
    let Some(full) = page_content(&super::pdf_layers::source_with_layers(path, &[]), page) else {
        return;
    };
    let key = |s: &dyn std::fmt::Debug| format!("{s:?}");
    let owner = |item: String| {
        layered.iter().position(|(_, pv)| {
            pv.paths.iter().any(|p| key(p) == item) || pv.texts.iter().any(|t| key(t) == item)
        })
    };
    let mut last: Option<usize> = None;
    let mut moves: Vec<(usize, PdfImage)> = Vec::new();
    for drawn in &full.order {
        match *drawn {
            Drawn::Path(i) => last = owner(key(&full.paths[i])).or(last),
            Drawn::Text(i) => last = owner(key(&full.texts[i])).or(last),
            Drawn::Image(i) => {
                let image = &full.images[i];
                if let Some(n) = layered.iter().position(|(_, pv)| pv.images.contains(image)) {
                    last = Some(n);
                } else if let (Some(n), Some(at)) = (last, base.images.iter().position(|b| b == image)) {
                    moves.push((n, base.images.remove(at)));
                }
            }
        }
    }
    for (n, image) in moves {
        layered[n].1.images.push(image);
    }
}

fn read_page(path: &str, page: &str) -> Option<PageVectors> {
    read_page_with(path, page, false)
}

fn read_page_with(path: &str, page: &str, images: bool) -> Option<PageVectors> {
    let bytes = super::pdf_raster::source_bytes(path)?;
    let pdf = Pdf::new(bytes).ok()?;
    let page_no = page.trim().parse::<usize>().ok()?;
    let page = pdf.pages().get(page_no.checked_sub(1)?)?;
    let (w, h) = page.render_dimensions();
    // Page points with y up → inches.
    let initial = Affine::scale(1.0 / 72.0) * page.initial_transform(false).to_kurbo();
    let cache = InterpreterCache::new();
    let mut context = Context::new(
        initial,
        Rect::new(0.0, 0.0, w as f64, h as f64),
        &cache,
        page.xref(),
        InterpreterSettings::default(),
    );
    let mut device = Collector {
        want_images: images,
        ..Collector::default()
    };
    interpret_page(page, &mut context, &mut device);
    device.finish_text();

    // Base font names in the order the content stream selects them; glyph
    // fonts are matched to them in their order of first use.
    let mut names: Vec<String> = Vec::new();
    let mut ops = page.typed_operations();
    while let Some(op) = ops.next() {
        if let TypedInstruction::TextFont(font) = op {
            let name = page
                .resources()
                .get_font(font.0)
                .and_then(|dict| dict.get::<Name>(b"BaseFont"))
                .map(|name| String::from_utf8_lossy(&name).into_owned())
                .unwrap_or_default();
            let name = name.split_once('+').map(|(_, rest)| rest.to_string()).unwrap_or(name);
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    let mut texts = device.texts;
    for text in &mut texts {
        let index = device.font_order.iter().position(|key| *key == text.font_key);
        text.text.font = index
            .and_then(|i| names.get(i).cloned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Text".to_string());
    }
    Some(PageVectors {
        paths: device.paths,
        texts: texts.into_iter().map(|t| t.text).collect(),
        images: device.images,
        order: device.order,
    })
}

struct PendingText {
    text: PdfText,
    font_key: u128,
    /// Baseline end, inches, for joining the next glyph.
    end: [f64; 2],
    em: f64,
    /// Tops of the capital letters seen so far, inches.
    caps: Vec<f64>,
}

#[derive(Default)]
struct Collector {
    paths: Vec<PdfPath>,
    texts: Vec<PendingText>,
    current: Option<PendingText>,
    font_order: Vec<u128>,
    want_images: bool,
    images: Vec<PdfImage>,
    order: Vec<Drawn>,
}

impl Collector {
    fn finish_text(&mut self) {
        if let Some(text) = self.current.take() {
            if !text.text.text.trim().is_empty() {
                self.order.push(Drawn::Text(self.texts.len()));
                self.texts.push(text);
            }
        }
    }
}

fn rgb(paint: &Paint<'_>) -> [u8; 3] {
    match paint {
        Paint::Color(color) => {
            let c = color.to_rgba().to_rgba8();
            [c[0], c[1], c[2]]
        }
        Paint::Pattern(_) => [0, 0, 0],
    }
}

fn pt(p: Point) -> [f64; 2] {
    [p.x, p.y]
}

fn subpaths(path: &BezPath, transform: Affine) -> Vec<SubPath> {
    let mut out: Vec<SubPath> = Vec::new();
    let mut start = Point::ZERO;
    let mut last = Point::ZERO;
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                let p = transform * p;
                out.push(SubPath { segments: Vec::new(), closed: false });
                start = p;
                last = p;
            }
            PathEl::LineTo(p) => {
                let p = transform * p;
                if let Some(sp) = out.last_mut() {
                    sp.segments.push(Segment::Line(pt(last), pt(p)));
                }
                last = p;
            }
            PathEl::QuadTo(c, p) => {
                let (c, p) = (transform * c, transform * p);
                let c1 = last + (c - last) * (2.0 / 3.0);
                let c2 = p + (c - p) * (2.0 / 3.0);
                if let Some(sp) = out.last_mut() {
                    sp.segments.push(Segment::Cubic(pt(last), pt(c1), pt(c2), pt(p)));
                }
                last = p;
            }
            PathEl::CurveTo(c1, c2, p) => {
                let (c1, c2, p) = (transform * c1, transform * c2, transform * p);
                if let Some(sp) = out.last_mut() {
                    sp.segments.push(Segment::Cubic(pt(last), pt(c1), pt(c2), pt(p)));
                }
                last = p;
            }
            PathEl::ClosePath => {
                if let Some(sp) = out.last_mut() {
                    if (last - start).hypot() > 1e-9 {
                        sp.segments.push(Segment::Line(pt(last), pt(start)));
                    }
                    sp.closed = true;
                }
                last = start;
            }
        }
    }
    out.retain(|sp| !sp.segments.is_empty());
    // A path drawn back to its start without a close operator is closed.
    for sp in &mut out {
        let (first, last) = (sp.segments[0].start(), sp.segments[sp.segments.len() - 1].end());
        if !sp.closed && (first[0] - last[0]).hypot(first[1] - last[1]) < 1e-9 {
            sp.closed = true;
        }
    }
    out
}

impl<'a> Device<'a> for Collector {
    fn set_soft_mask(&mut self, _: Option<SoftMask<'a>>) {}
    fn set_blend_mode(&mut self, _: BlendMode) {}
    fn draw_path(&mut self, path: &BezPath, transform: Affine, paint: &Paint<'a>, mode: &PathDrawMode) {
        self.finish_text();
        let subpaths = subpaths(path, transform);
        if subpaths.is_empty() {
            return;
        }
        let (stroke, fill) = match mode {
            PathDrawMode::Stroke(props) => {
                // Width in points: the user-space width through the transform
                // (which is in inches), back to points.
                let scale = transform.determinant().abs().sqrt() * 72.0;
                (Some((rgb(paint), props.line_width as f64 * scale)), None)
            }
            PathDrawMode::Fill(_) => (None, Some(rgb(paint))),
        };
        self.order.push(Drawn::Path(self.paths.len()));
        self.paths.push(PdfPath { subpaths, stroke, fill });
    }
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_glyph(
        &mut self,
        glyph: &Glyph<'a>,
        transform: Affine,
        glyph_transform: Affine,
        paint: &Paint<'a>,
        _: &GlyphDrawMode,
    ) {
        let Glyph::Outline(outline) = glyph else {
            return;
        };
        let full = transform * glyph_transform;
        let origin = full * Point::ZERO;
        // Outlines are in 1000 units per em.
        let em_vec = full * Point::new(0.0, 1000.0) - origin;
        let em = em_vec.hypot();
        if em <= 0.0 {
            return;
        }
        let advance = outline.advance_width().unwrap_or(0.0) as f64;
        let adv_vec = full * Point::new(advance, 0.0) - origin;
        let run_dir = full * Point::new(1000.0, 0.0) - origin;
        let rotation = run_dir.y.atan2(run_dir.x);
        let text = outline
            .as_unicode()
            .map(|s| match s {
                hayro::hayro_interpret::hayro_cmap::BfString::Char(c) => c.to_string(),
                hayro::hayro_interpret::hayro_cmap::BfString::String(s) => s,
            })
            .unwrap_or_default();
        let key = outline.font_cache_key();
        if !self.font_order.contains(&key) {
            self.font_order.push(key);
        }
        let bounds = outline.outline().bounding_box();
        let glyph_top = text
            .chars()
            .any(|c| c.is_uppercase())
            .then(|| bounds.y1 / 1000.0 * em);
        let origin = pt(origin);
        let joins = self.current.as_ref().is_some_and(|run| {
            run.font_key == key
                && (run.em - em).abs() < 1e-6
                && (run.text.rotation - rotation).abs() < 1e-6
                && ((run.end[0] - origin[0]).powi(2) + (run.end[1] - origin[1]).powi(2)).sqrt()
                    < 0.3 * em
        });
        if !joins {
            self.finish_text();
            self.current = Some(PendingText {
                text: PdfText {
                    text: String::new(),
                    font: String::new(),
                    origin,
                    cap_height: 0.0,
                    width: 0.0,
                    rotation,
                    color: rgb(paint),
                },
                font_key: key,
                end: origin,
                em,
                caps: Vec::new(),
            });
        }
        if let Some(run) = self.current.as_mut() {
            run.text.text.push_str(&text);
            if let Some(top) = glyph_top {
                run.caps.push(top);
                // Capital height: the median capital top, so round letters'
                // overshoot (O, G) does not lift it.
                let mut caps = run.caps.clone();
                caps.sort_by(f64::total_cmp);
                run.text.cap_height = caps[caps.len() / 2];
            }
            run.end = [origin[0] + adv_vec.x, origin[1] + adv_vec.y];
            run.text.width = ((run.end[0] - run.text.origin[0]).powi(2)
                + (run.end[1] - run.text.origin[1]).powi(2))
            .sqrt();
        }
    }
    fn draw_image(&mut self, image: Image<'a, '_>, transform: Affine) {
        let Image::Raster(raster) = image else {
            return;
        };
        if !self.want_images {
            return;
        }
        let mut decoded: Option<PdfImage> = None;
        raster.with_rgba(
            |data, alpha| {
                use hayro::hayro_interpret::ImageData;
                let (w, h) = (data.width(), data.height());
                let n = (w * h) as usize;
                let mut rgba = vec![255u8; n * 4];
                match &data {
                    ImageData::Rgb(rgb) => {
                        for i in 0..n {
                            rgba[i * 4..i * 4 + 3].copy_from_slice(&rgb.data[i * 3..i * 3 + 3]);
                        }
                    }
                    ImageData::Luma(luma) => {
                        for i in 0..n {
                            rgba[i * 4..i * 4 + 3].fill(luma.data[i]);
                        }
                    }
                }
                if let Some(a) = alpha.filter(|a| a.width == w && a.height == h) {
                    for i in 0..n {
                        rgba[i * 4 + 3] = a.data[i];
                    }
                }
                // The transform maps pixel space (y down) onto the page.
                let at = |x: f64, y: f64| pt(transform * Point::new(x, y));
                let (wf, hf) = (w as f64, h as f64);
                let (o, x1, y1) = (at(0.0, hf), at(wf, hf), at(0.0, 0.0));
                decoded = Some(PdfImage {
                    rgba,
                    width: w,
                    height: h,
                    origin: o,
                    u: [(x1[0] - o[0]) / w as f64, (x1[1] - o[1]) / w as f64],
                    v: [(y1[0] - o[0]) / h as f64, (y1[1] - o[1]) / h as f64],
                });
            },
            None,
        );
        if let Some(image) = decoded {
            self.order.push(Drawn::Image(self.images.len()));
            self.images.push(image);
        }
    }
    fn pop_clip_path(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

/// A point of a segment at parameter `t` (0..1).
pub fn bezier(seg: &Segment, t: f64) -> [f64; 2] {
    match seg {
        Segment::Line(a, b) => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
        Segment::Cubic(p0, p1, p2, p3) => {
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            [
                a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0],
                a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1],
            ]
        }
    }
}

/// Centre and radius when a closed loop of cubics traces a circle.
pub fn circle_of(sp: &SubPath) -> Option<([f64; 2], f64)> {
    if !sp.closed || sp.segments.len() < 4 {
        return None;
    }
    let cubics: Vec<&Segment> = sp
        .segments
        .iter()
        .filter(|s| matches!(s, Segment::Cubic(..)))
        .collect();
    if cubics.len() != sp.segments.len() && sp.segments.len() - cubics.len() > 1 {
        return None;
    }
    let starts: Vec<[f64; 2]> = cubics.iter().map(|s| s.start()).collect();
    let n = starts.len() as f64;
    let c = [
        starts.iter().map(|p| p[0]).sum::<f64>() / n,
        starts.iter().map(|p| p[1]).sum::<f64>() / n,
    ];
    let dist = |p: [f64; 2]| ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2)).sqrt();
    let r = starts.iter().map(|p| dist(*p)).sum::<f64>() / n;
    if r <= 0.0 {
        return None;
    }
    let tol = r * 2e-3;
    let on_circle = cubics.iter().all(|s| {
        [0.0, 0.25, 0.5, 0.75].iter().all(|t| (dist(bezier(s, *t)) - r).abs() <= tol)
    });
    on_circle.then_some((c, r))
}

// ── Snapping ────────────────────────────────────────────────────────────────

static PDF_OSNAP: AtomicBool = AtomicBool::new(true);

/// PDFOSNAP: whether object snaps find the geometry inside PDF underlays.
pub fn pdf_osnap() -> bool {
    PDF_OSNAP.load(Ordering::Relaxed)
}

pub fn set_pdf_osnap(on: bool) {
    PDF_OSNAP.store(on, Ordering::Relaxed);
}

/// Most snap points one underlay contributes.
// ponytail: a flat cap; a spatial index over the page segments if large
// drawings need every point.
const MAX_SNAP_POINTS: usize = 20_000;

/// Endpoint and midpoint snaps of the underlay's PDF geometry, in world
/// space, when PDFOSNAP is on and the page is shown.
pub fn underlay_snap_points(
    u: &codec::entities::Underlay,
    document: &codec::CadDocument,
) -> Vec<(glam::DVec3, crate::scene::model::wire_model::SnapHint)> {
    use crate::scene::model::wire_model::SnapHint;
    if !pdf_osnap() || !u.flags.contains(codec::entities::UnderlayDisplayFlags::ON) {
        return Vec::new();
    }
    let Some(def) = crate::entities::underlay::definition(u, document) else {
        return Vec::new();
    };
    let source = super::pdf_layers::underlay_source(u, &def.file_path);
    let Some(vectors) = page_vectors(&source, crate::entities::underlay::page_of(def)) else {
        return Vec::new();
    };
    let world = |p: [f64; 2]| {
        let w = crate::entities::underlay::local_to_world(u, p);
        glam::DVec3::new(w[0], w[1], w[2])
    };
    let mut out = Vec::new();
    'paths: for path in &vectors.paths {
        for sp in &path.subpaths {
            // A PDF circle offers its centre and quadrants, as a circle does.
            if let Some((c, r)) = circle_of(sp) {
                out.push((world(c), SnapHint::Center));
                for (dx, dy) in [(r, 0.0), (0.0, r), (-r, 0.0), (0.0, -r)] {
                    out.push((world([c[0] + dx, c[1] + dy]), SnapHint::Quadrant));
                }
            }
            for seg in &sp.segments {
                if out.len() >= MAX_SNAP_POINTS {
                    break 'paths;
                }
                out.push((world(seg.start()), SnapHint::Endpoint));
                out.push((world(seg.end()), SnapHint::Endpoint));
                if let Segment::Line(a, b) = seg {
                    out.push((
                        world([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]),
                        SnapHint::Midpoint,
                    ));
                }
            }
        }
    }
    out
}

/// Most points of the snap-only geometry wire of one underlay.
// ponytail: flat cap; a spatial index over page segments for huge drawings.
const MAX_GEOMETRY_POINTS: usize = 200_000;

/// One piece of an underlay's PDF geometry for snapping: a world polyline
/// (curves flattened), and for a circle its centre and radius.
pub struct SnapPiece {
    pub points: Vec<[f64; 3]>,
    pub circle: Option<([f64; 3], f64)>,
}

/// The underlay's PDF geometry for nearest, intersection, perpendicular and
/// centre snaps: each straight segment, each curve and each circle its own
/// piece, so pieces cross each other as separate objects do. Empty when
/// PDFOSNAP is off or the page is not shown.
pub fn underlay_snap_geometry(
    u: &codec::entities::Underlay,
    document: &codec::CadDocument,
) -> Vec<SnapPiece> {
    if !pdf_osnap() || !u.flags.contains(codec::entities::UnderlayDisplayFlags::ON) {
        return Vec::new();
    }
    let Some(def) = crate::entities::underlay::definition(u, document) else {
        return Vec::new();
    };
    if def.unloaded {
        return Vec::new();
    }
    let source = super::pdf_layers::underlay_source(u, &def.file_path);
    let Some(vectors) = page_vectors(&source, crate::entities::underlay::page_of(def)) else {
        return Vec::new();
    };
    let world = |p: [f64; 2]| crate::entities::underlay::local_to_world(u, p);
    // A circle stays one only while the placement scales both axes alike.
    let uniform = (u.x_scale.abs() - u.y_scale.abs()).abs() <= 1e-9 * u.x_scale.abs().max(1e-12);
    let flatten = |seg: &Segment, points: &mut Vec<[f64; 3]>| {
        let steps = if matches!(seg, Segment::Cubic(..)) { 12 } else { 1 };
        for k in 1..=steps {
            points.push(world(bezier(seg, k as f64 / steps as f64)));
        }
    };
    let mut out: Vec<SnapPiece> = Vec::new();
    let mut total = 0usize;
    for path in &vectors.paths {
        for sp in &path.subpaths {
            if total >= MAX_GEOMETRY_POINTS {
                return out;
            }
            if let (Some((c, r)), true) = (circle_of(sp), uniform) {
                let mut points = vec![world(sp.segments[0].start())];
                for seg in &sp.segments {
                    flatten(seg, &mut points);
                }
                total += points.len();
                out.push(SnapPiece { points, circle: Some((world(c), r * u.x_scale.abs())) });
                continue;
            }
            for seg in &sp.segments {
                let mut points = vec![world(seg.start())];
                flatten(seg, &mut points);
                total += points.len();
                out.push(SnapPiece { points, circle: None });
            }
        }
    }
    out
}
