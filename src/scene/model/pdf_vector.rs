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

/// How a subpath of a PDF path ends.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SubClose {
    /// Closed by the close operator (`h`, `s`, `b`, `re`).
    pub op: bool,
    /// Its last segment is the line that operator added back to the start.
    pub line: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PdfPath {
    pub subpaths: Vec<SubPath>,
    /// Stroke colour (RGB) and width in points, for stroked paths.
    pub stroke: Option<([u8; 3], f64)>,
    /// Fill colour (RGB), for filled paths.
    pub fill: Option<[u8; 3]>,
    /// The stroke colour and width (points) current when the path was
    /// painted, stroked or not; `None` when the page could not be followed.
    pub pen: Option<([u8; 3], f64)>,
    /// Per subpath, how it ends (empty when not known).
    pub closes: Vec<SubClose>,
    /// The clipping polygon (page inches, convex) the path reaches out of;
    /// `None` when the path lies inside the clip or the clip is no polygon.
    pub clip: Option<Vec<[f64; 2]>>,
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
    /// The font size (one em), inches.
    pub em: f64,
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
    /// For each drawn item, how many optional-content sections (`/OC …
    /// BDC`) had begun before it.
    pub oc_marks: Vec<usize>,
    /// Optional-content sections the page begins in all.
    pub oc_sections: usize,
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
/// then each named layer that is not in `hidden`. Layers are taken by name
/// (two groups of one name are one layer). An item belongs to the innermost
/// layer that hides it: of the layers whose turning off removes it, the one
/// that removes the least. What only several layers together decide
/// (membership dictionaries) goes to the last of its layers that shows it alone.
pub fn page_content_by_layer(
    path: &str,
    page: &str,
    hidden: &[String],
) -> Option<Vec<(Option<String>, PageVectors)>> {
    use super::pdf_layers::source_with_layers;
    let layers = super::pdf_layers::layers(path);
    if layers.is_empty() {
        return Some(vec![(None, page_content(path, page)?)]);
    }
    let mut names: Vec<String> = Vec::new();
    for layer in &layers {
        if !names.contains(&layer.name) {
            names.push(layer.name.clone());
        }
    }
    let read = |off: &[String]| page_content(&source_with_layers(path, off), page);
    let shown = read(hidden)?;
    let visible: Vec<&String> = names.iter().filter(|n| !hidden.contains(n)).collect();
    // The group of each drawn item: 0 for no layer, else 1 + its index in
    // `visible`. The reference puts an item on the layer whose section
    // opened last before it (a section left keeps its layer current). When
    // every section is read in order, that sequence decides; otherwise the
    // layers are told apart by turning them off.
    let sections = oc_section_layers(path, page).filter(|s| s.len() == shown.oc_sections);
    let group_of: Vec<usize> = if let Some(sections) = sections {
        let mut current: Vec<Option<String>> = Vec::with_capacity(sections.len() + 1);
        current.push(None);
        for section in sections {
            let last = current.last().cloned().flatten();
            current.push(section.or(last));
        }
        (0..shown.order.len())
            .map(|k| {
                let layer = shown.oc_marks.get(k).and_then(|m| current.get(*m)).cloned();
                layer
                    .flatten()
                    .and_then(|l| visible.iter().position(|n| **n == l))
                    .map_or(0, |n| n + 1)
            })
            .collect()
    } else {
        groups_by_removal(&shown, &names, &visible, hidden, &read)?
    };
    let mut groups: Vec<PageVectors> =
        (0..=visible.len()).map(|_| PageVectors::default()).collect();
    for (drawn, g) in shown.order.iter().zip(group_of) {
        let group = &mut groups[g];
        match *drawn {
            Drawn::Path(i) => {
                group.order.push(Drawn::Path(group.paths.len()));
                group.paths.push(shown.paths[i].clone());
            }
            Drawn::Text(i) => {
                group.order.push(Drawn::Text(group.texts.len()));
                group.texts.push(shown.texts[i].clone());
            }
            Drawn::Image(i) => {
                group.order.push(Drawn::Image(group.images.len()));
                group.images.push(shown.images[i].clone());
            }
        }
    }
    let mut groups = groups.into_iter();
    let base = groups.next().unwrap_or_default();
    let mut out: Vec<(Option<String>, PageVectors)> = visible
        .iter()
        .map(|n| Some((*n).clone()))
        .zip(groups)
        .collect();
    out.insert(0, (None, base));
    Some(out)
}

/// The group of each item `shown` draws (as in [`page_content_by_layer`]),
/// told apart by reading the page with layers turned off.
fn groups_by_removal(
    shown: &PageVectors,
    names: &[String],
    visible: &[&String],
    hidden: &[String],
    read: &dyn Fn(&[String]) -> Option<PageVectors>,
) -> Option<Vec<usize>> {
    let key = |s: &dyn std::fmt::Debug| format!("{s:?}");
    // A path filled and stroked by one operator is keyed by its fill: a
    // read that hides some layer's paths does not merge such pairs (see
    // `merge_fill_and_stroke`), and its fill half must still match.
    let path_key = |p: &PdfPath| match p.fill {
        Some(_) if p.stroke.is_some() => key(&PdfPath {
            stroke: None,
            ..p.clone()
        }),
        _ => key(p),
    };
    let keys = |pv: &PageVectors| -> HashMap<String, usize> {
        let mut out: HashMap<String, usize> = HashMap::new();
        let texts = pv.texts.iter().map(|t| key(t));
        for item in pv.paths.iter().map(path_key).chain(texts) {
            *out.entry(item).or_default() += 1;
        }
        out
    };
    let less = |a: &HashMap<String, usize>, b: &HashMap<String, usize>| {
        a.iter()
            .filter_map(|(k, n)| {
                let left = n.saturating_sub(b.get(k).copied().unwrap_or(0));
                (left > 0).then(|| (k.clone(), left))
            })
            .collect::<HashMap<String, usize>>()
    };
    let shown_keys = keys(shown);
    let none_on = read(names)?;
    let mut unlayered = keys(&none_on);
    // What turning each layer off (as well) removes.
    let mut removed: Vec<(HashMap<String, usize>, Vec<PdfImage>, usize)> = Vec::new();
    for name in visible {
        let mut off = hidden.to_vec();
        off.push((*name).clone());
        let (map, images) = match read(&off) {
            Some(without) => {
                let images: Vec<PdfImage> = shown
                    .images
                    .iter()
                    .filter(|i| !without.images.contains(i))
                    .cloned()
                    .collect();
                (less(&shown_keys, &keys(&without)), images)
            }
            None => (HashMap::new(), Vec::new()),
        };
        let size = map.values().sum::<usize>() + images.len();
        removed.push((map, images, size));
    }
    // What each layer's removal holds, kept whole for nesting tests.
    let holds: Vec<HashMap<String, usize>> =
        removed.iter().map(|(map, _, _)| map.clone()).collect();
    let image_group: Vec<usize> = shown
        .images
        .iter()
        .map(|image| {
            removed
                .iter()
                .enumerate()
                .filter(|(_, (_, images, _))| images.contains(image))
                .min_by_key(|(_, (_, _, size))| *size)
                .map_or(0, |(n, _)| n + 1)
        })
        .collect();
    // What each layer shows alone, read only when needed.
    let mut alone: Vec<Option<HashMap<String, usize>>> = vec![None; visible.len()];
    let none_keys = unlayered.clone();
    let mut owner_of = |item: String| -> Option<usize> {
        if let Some(n) = unlayered.get_mut(&item).filter(|n| **n > 0) {
            *n -= 1;
            return None;
        }
        let best = removed
            .iter()
            .enumerate()
            .filter(|(_, (map, _, _))| map.get(&item).is_some_and(|n| *n > 0))
            .min_by_key(|(_, (_, _, size))| *size)
            .map(|(n, _)| n);
        if let Some(n) = best {
            if let Some(count) = removed[n].0.get_mut(&item) {
                *count -= 1;
            }
            return Some(n);
        }
        for (n, name) in visible.iter().enumerate().rev() {
            if alone[n].is_none() {
                let others: Vec<String> = names.iter().filter(|m| m != name).cloned().collect();
                let only = read(&others).map(|pv| less(&keys(&pv), &none_keys));
                alone[n] = Some(only.unwrap_or_default());
            }
            let count = alone[n].as_mut().and_then(|m| m.get_mut(&item));
            if let Some(count) = count.filter(|c| **c > 0) {
                *count -= 1;
                return Some(n);
            }
        }
        None
    };
    // The layer an item lands on follows the reference's reading of the
    // marked content: the layer last opened stays current. An item no layer
    // holds goes with the layer of the item drawn before it, and an item of
    // an outer layer drawn after an inner one closed stays on the inner one.
    let mut last: Option<(usize, String)> = None;
    let mut place = |owner: Option<usize>, item: String| -> usize {
        let g = match (owner, &last) {
            (None, Some((m, _))) => Some(*m),
            (Some(n), Some((m, previous))) if *m != n && holds[n].contains_key(previous) => {
                Some(*m)
            }
            (owner, _) => owner,
        };
        if let Some(g) = g {
            last = Some((g, item));
        }
        g.map_or(0, |g| g + 1)
    };
    let group_of = shown
        .order
        .iter()
        .map(|drawn| match *drawn {
            Drawn::Path(i) => {
                let item = path_key(&shown.paths[i]);
                place(owner_of(item.clone()), item)
            }
            Drawn::Text(i) => {
                let item = key(&shown.texts[i]);
                place(owner_of(item.clone()), item)
            }
            Drawn::Image(i) => place(image_group[i].checked_sub(1), format!("image {i}")),
        })
        .collect();
    Some(group_of)
}

fn read_page(path: &str, page: &str) -> Option<PageVectors> {
    read_page_with(path, page, false)
}

fn read_page_with(path: &str, page: &str, images: bool) -> Option<PageVectors> {
    let bytes = super::pdf_raster::source_bytes(path)?;
    let pdf = super::pdf_password::open(path, bytes).ok()?;
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
        visible: vec![Rect::new(0.0, 0.0, w as f64 / 72.0, h as f64 / 72.0)],
        ..Collector::default()
    };
    interpret_page(page, &mut context, &mut device);
    device.finish_text();
    assign_pens(page, initial, &mut device);
    merge_fill_and_stroke(page, &mut device);

    // Base font names in the order the content stream selects them; glyph
    // fonts are matched to them in their order of first use.
    let mut names: Vec<String> = Vec::new();
    let mut ops = page.typed_operations();
    while let Some(op) = ops.next() {
        if let TypedInstruction::TextFont(font) = op {
            let dict = page.resources().get_font(font.0);
            let name = dict
                .as_ref()
                .and_then(|dict| dict.get::<Name>(b"BaseFont"))
                .map(|name| String::from_utf8_lossy(&name).into_owned())
                .unwrap_or_default();
            let name = name
                .split_once('+')
                .map(|(_, rest)| rest.to_string())
                .unwrap_or(name);
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    let mut texts = device.texts;
    for text in &mut texts {
        let index = device
            .font_order
            .iter()
            .position(|key| *key == text.font_key);
        text.text.font = index
            .and_then(|i| names.get(i).cloned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Text".to_string());
        // Capital height as a size share; PDF import measures its own (see
        // `pdf_import`).
        text.text.cap_height = 0.72 * text.em;
        text.text.em = text.em;
    }
    Some(PageVectors {
        paths: device.paths,
        texts: texts.into_iter().map(|t| t.text).collect(),
        images: device.images,
        order: device.order,
        oc_marks: device.oc_marks,
        oc_sections: device.oc_sections,
    })
}

/// A path filled and stroked by one operator (`B`, `b*` …) is one path
/// with both a fill and a stroke, as the reference reads it (its fill, at
/// the stroke's weight). The interpreter draws such a path twice; the page's
/// operators tell those pairs from a fill followed by a separate stroke of
/// the same path. Nothing is merged when the drawn paths do not match the
/// operators one for one (paths drawn by patterns or glyph procedures, or
/// left out by a hidden layer).
fn merge_fill_and_stroke(
    page: &hayro::hayro_syntax::page::Page<'_>,
    device: &mut Collector,
) {
    let mut calls: Vec<usize> = Vec::new();
    walk_operators(page, &mut |op, _| {
        calls.push(match op {
            TypedInstruction::StrokePath(_)
            | TypedInstruction::CloseAndStrokePath(_)
            | TypedInstruction::FillPathNonZero(_)
            | TypedInstruction::FillPathNonZeroCompatibility(_)
            | TypedInstruction::FillPathEvenOdd(_) => 1,
            TypedInstruction::FillAndStrokeNonZero(_)
            | TypedInstruction::FillAndStrokeEvenOdd(_)
            | TypedInstruction::CloseFillAndStrokeNonZero(_)
            | TypedInstruction::CloseFillAndStrokeEvenOdd(_) => 2,
            _ => return,
        });
    });
    if calls.iter().sum::<usize>() != device.path_calls.len() {
        return;
    }
    let mut gone = vec![false; device.paths.len()];
    let mut at = 0;
    for count in calls {
        if count == 2 {
            let pair = (device.path_calls.get(at), device.path_calls.get(at + 1));
            if let (Some(Some(f)), Some(Some(s))) = pair {
                let (f, s) = (*f, *s);
                if device.paths[f].fill.is_some()
                    && device.paths[s].stroke.is_some()
                    && device.paths[f].subpaths == device.paths[s].subpaths
                {
                    device.paths[f].stroke = device.paths[s].stroke;
                    gone[s] = true;
                }
            }
        }
        at += count;
    }
    if !gone.contains(&true) {
        return;
    }
    let mut new_index = vec![0usize; gone.len()];
    let mut kept = Vec::new();
    for (k, path) in std::mem::take(&mut device.paths).into_iter().enumerate() {
        new_index[k] = kept.len();
        if !gone[k] {
            kept.push(path);
        }
    }
    device.paths = kept;
    let (mut order, mut marks) = (Vec::new(), Vec::new());
    for (drawn, mark) in device.order.iter().zip(&device.oc_marks) {
        match *drawn {
            Drawn::Path(i) if gone[i] => continue,
            Drawn::Path(i) => order.push(Drawn::Path(new_index[i])),
            other => order.push(other),
        }
        marks.push(*mark);
    }
    device.order = order;
    device.oc_marks = marks;
}

/// The layer each optional-content section of a page's own content stream
/// opens, in order: a group's name; for a membership dictionary, its last
/// group's when it is an object of its own, its first when written out in
/// the resources (as the reference reads them); `None` when it names no
/// group.
fn oc_section_layers(path: &str, page: &str) -> Option<Vec<Option<String>>> {
    use hayro::hayro_syntax::object::{Array, Dict, ObjectIdentifier};
    let bytes = super::pdf_raster::source_bytes(path)?;
    let pdf = super::pdf_password::open(path, bytes).ok()?;
    let xref = pdf.xref();
    let page = pdf.pages().get(page.trim().parse::<usize>().ok()?.checked_sub(1)?)?;
    let group_name = |dict: &Dict<'_>| {
        dict.get::<hayro::hayro_syntax::object::String<'_>>(b"Name")
            .map(|s| super::pdf_layers::text_string(s.as_bytes()))
    };
    let mut out = Vec::new();
    walk_operators(&page, &mut |op, resources| {
        let TypedInstruction::BeginMarkedContentWithProperties(bdc) = op else { return };
        if bdc.0.as_ref() != b"OC" {
            return;
        }
        // A form's own properties, else the page's.
        let properties = match bdc.1.clone().into_name() {
            Some(n) if resources.properties.contains_key(n.as_ref()) => &resources.properties,
            _ => &page.resources().properties,
        };
        let name = bdc.1.clone().into_name();
        let indirect = name.as_ref().and_then(|n| properties.get_ref(n.as_ref())).is_some();
        let dict = name.and_then(|n| properties.get::<Dict<'_>>(n.as_ref()));
        let layer = dict.and_then(|dict| {
            if dict.get::<Name<'_>>(b"Type").is_some_and(|t| t.as_ref() == b"OCMD") {
                let groups: Vec<Dict<'_>> = match dict.get::<Array<'_>>(b"OCGs") {
                    Some(array) => array
                        .raw_iter()
                        .filter_map(|item| item.as_obj_ref())
                        .filter_map(|r| xref.get::<Dict<'_>>(ObjectIdentifier::from(r)))
                        .collect(),
                    None => dict.get::<Dict<'_>>(b"OCGs").into_iter().collect(),
                };
                let group = if indirect { groups.last() } else { groups.first() };
                group.and_then(group_name)
            } else {
                group_name(&dict)
            }
        });
        out.push(layer);
    });
    Some(out)
}

/// Every operator of a page's content in the order the interpreter runs
/// them, a form XObject's operators where it is drawn (with the resources
/// they read names from).
fn walk_operators<'a>(
    page: &hayro::hayro_syntax::page::Page<'a>,
    visit: &mut dyn FnMut(&TypedInstruction<'_, '_>, &hayro::hayro_syntax::page::Resources<'a>),
) {
    walk_steps(page, &mut |step, resources| {
        if let Step::Op(op) = step {
            visit(op, resources);
        }
    });
}

/// One step of [`walk_steps`].
enum Step<'s, 'b, 'a> {
    Op(&'s TypedInstruction<'b, 'a>),
    /// A form begins, with its matrix.
    FormStart(Affine),
    FormEnd,
}

/// [`walk_operators`], also told where each form begins and ends.
fn walk_steps<'a>(
    page: &hayro::hayro_syntax::page::Page<'a>,
    visit: &mut dyn FnMut(Step<'_, '_, '_>, &hayro::hayro_syntax::page::Resources<'a>),
) {
    use hayro::hayro_syntax::content::TypedIter;
    use hayro::hayro_syntax::object::{Array, Dict};
    use hayro::hayro_syntax::page::Resources;
    fn walk<'a>(
        data: &[u8],
        resources: &Resources<'a>,
        depth: usize,
        visit: &mut dyn FnMut(Step<'_, '_, '_>, &Resources<'a>),
    ) {
        let mut ops = TypedIter::new(data);
        while let Some(op) = ops.next() {
            visit(Step::Op(&op), resources);
            let TypedInstruction::XObject(x) = &op else { continue };
            // Nested forms are followed a few levels deep.
            if depth >= 8 {
                continue;
            }
            let Some(stream) = resources.get_x_object(&x.0) else { continue };
            if !stream.dict().get::<Name<'_>>(b"Subtype").is_some_and(|t| t.as_ref() == b"Form") {
                continue;
            }
            let own = match stream.dict().get::<Dict<'_>>(b"Resources") {
                Some(dict) => Resources::from_parent(dict, resources.clone()),
                None => resources.clone(),
            };
            if let Ok(data) = stream.decoded() {
                let m: Vec<f64> = stream
                    .dict()
                    .get::<Array<'_>>(b"Matrix")
                    .map(|a| a.iter::<f64>().collect())
                    .unwrap_or_default();
                let matrix = match m.as_slice() {
                    [a, b, c, d, e, f] => Affine::new([*a, *b, *c, *d, *e, *f]),
                    _ => Affine::IDENTITY,
                };
                visit(Step::FormStart(matrix), &own);
                walk(&data, &own, depth + 1, visit);
                visit(Step::FormEnd, &own);
            }
        }
    }
    if let Some(data) = page.page_stream() {
        walk(data, page.resources(), 0, visit);
    }
}

/// The stroke colour and width current when each path the interpreter drew
/// was painted, followed through the page's operators (the interpreter hands
/// a fill only its fill colour). Left unset when the drawn paths do not match
/// the painting operators one for one.
fn assign_pens(
    page: &hayro::hayro_syntax::page::Page<'_>,
    initial: Affine,
    device: &mut Collector,
) {
    use hayro::hayro_syntax::object::{Dict, Number, Object, Stream};
    #[derive(Clone)]
    struct State {
        ctm: Affine,
        width: f64,
        rgb: [u8; 3],
        /// Components of the stroke colour space (0 for a pattern).
        components: usize,
    }
    fn to_rgb(v: &[f64]) -> [u8; 3] {
        let c = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        match v {
            [g] => [c(*g); 3],
            [r, g, b] => [c(*r), c(*g), c(*b)],
            [cy, m, y, k] => [
                c((1.0 - cy) * (1.0 - k)),
                c((1.0 - m) * (1.0 - k)),
                c((1.0 - y) * (1.0 - k)),
            ],
            _ => [0, 0, 0],
        }
    }
    fn components(name: &Name<'_>, resources: &hayro::hayro_syntax::page::Resources<'_>) -> usize {
        match name.as_ref() {
            b"DeviceGray" | b"G" | b"CalGray" => 1,
            b"DeviceRGB" | b"RGB" => 3,
            b"DeviceCMYK" | b"CMYK" => 4,
            b"Pattern" => 0,
            _ => match resources.get_color_space(name) {
                Some(Object::Array(array)) => {
                    match array.iter::<Name<'_>>().next().as_ref().map(|f| f.as_ref()) {
                        Some(b"ICCBased") => array
                            .iter::<Stream<'_>>()
                            .next()
                            .and_then(|s| s.dict().get::<usize>(b"N"))
                            .unwrap_or(3),
                        Some(b"CalRGB") | Some(b"Lab") => 3,
                        Some(b"Pattern") => 0,
                        _ => 1,
                    }
                }
                Some(Object::Name(n)) if n.as_ref() == b"Pattern" => 0,
                _ => 3,
            },
        }
    }
    let f = |v: &Number| v.as_f64();
    let pen = |s: &State| (s.rgb, s.width * s.ctm.determinant().abs().sqrt());
    let mut stack = vec![State {
        ctm: Affine::IDENTITY,
        width: 1.0,
        rgb: [0, 0, 0],
        components: 1,
    }];
    // A form runs in a saved state its own `Q` cannot pop.
    let mut floors: Vec<usize> = Vec::new();
    // Per drawn path: the pen, and for a fill the path in user space with
    // its transformation matrix.
    type Fill = Option<(BezPath, Affine)>;
    let mut calls: Vec<(([u8; 3], f64), Fill)> = Vec::new();
    let mut path = BezPath::new();
    let mut current = Point::ZERO;
    walk_steps(page, &mut |step, resources| {
        let op = match step {
            Step::Op(op) => op,
            Step::FormStart(matrix) => {
                let mut state = stack[stack.len() - 1].clone();
                state.ctm = state.ctm * matrix;
                floors.push(stack.len());
                stack.push(state);
                return;
            }
            Step::FormEnd => {
                if let Some(floor) = floors.pop() {
                    stack.truncate(floor.max(1));
                }
                return;
            }
        };
        let floor = floors.last().map_or(1, |f| f + 1);
        let top = stack.len() - 1;
        match op {
            TypedInstruction::SaveState(_) => stack.push(stack[top].clone()),
            TypedInstruction::RestoreState(_) => {
                if stack.len() > floor {
                    stack.pop();
                }
            }
            TypedInstruction::Transform(t) => {
                let m = Affine::new([f(&t.0), f(&t.1), f(&t.2), f(&t.3), f(&t.4), f(&t.5)]);
                stack[top].ctm = stack[top].ctm * m;
            }
            TypedInstruction::LineWidth(w) => stack[top].width = f(&w.0),
            TypedInstruction::SetGraphicsState(gs) => {
                let width = resources
                    .get_ext_g_state(&gs.0)
                    .and_then(|d: Dict<'_>| d.get::<f64>(b"LW"));
                if let Some(width) = width {
                    stack[top].width = width;
                }
            }
            TypedInstruction::StrokeColorDeviceGray(g) => {
                (stack[top].rgb, stack[top].components) = (to_rgb(&[f(&g.0)]), 1)
            }
            TypedInstruction::StrokeColorDeviceRgb(c) => {
                (stack[top].rgb, stack[top].components) =
                    (to_rgb(&[f(&c.0), f(&c.1), f(&c.2)]), 3)
            }
            TypedInstruction::StrokeColorCmyk(c) => {
                (stack[top].rgb, stack[top].components) =
                    (to_rgb(&[f(&c.0), f(&c.1), f(&c.2), f(&c.3)]), 4)
            }
            TypedInstruction::ColorSpaceStroke(cs) => {
                stack[top].components = components(cs.0, resources);
                stack[top].rgb = [0, 0, 0];
            }
            TypedInstruction::StrokeColor(c) => {
                if stack[top].components != 0 {
                    let v: Vec<f64> = c.0.iter().map(f).collect();
                    stack[top].rgb = to_rgb(&v);
                }
            }
            TypedInstruction::StrokeColorNamed(c) => {
                let v: Vec<f64> = c.0.iter().map(f).collect();
                stack[top].rgb = if stack[top].components == 0 {
                    [0, 0, 0]
                } else {
                    to_rgb(&v)
                };
            }
            TypedInstruction::MoveTo(m) => {
                let p = Point::new(f(&m.0), f(&m.1));
                path.move_to(p);
                current = p;
            }
            TypedInstruction::LineTo(m) => {
                let p = Point::new(f(&m.0), f(&m.1));
                path.line_to(p);
                current = p;
            }
            TypedInstruction::CubicTo(c) => {
                let p = Point::new(f(&c.4), f(&c.5));
                path.curve_to(Point::new(f(&c.0), f(&c.1)), Point::new(f(&c.2), f(&c.3)), p);
                current = p;
            }
            TypedInstruction::CubicStartTo(c) => {
                let p = Point::new(f(&c.2), f(&c.3));
                path.curve_to(current, Point::new(f(&c.0), f(&c.1)), p);
                current = p;
            }
            TypedInstruction::CubicEndTo(c) => {
                let p = Point::new(f(&c.2), f(&c.3));
                path.curve_to(Point::new(f(&c.0), f(&c.1)), p, p);
                current = p;
            }
            TypedInstruction::ClosePath(_) => path.close_path(),
            TypedInstruction::RectPath(r) => {
                let (x, y, w, h) = (f(&r.0), f(&r.1), f(&r.2), f(&r.3));
                path.move_to((x, y));
                path.line_to((x + w, y));
                path.line_to((x + w, y + h));
                path.line_to((x, y + h));
                path.close_path();
                current = Point::new(x, y);
            }
            TypedInstruction::EndPath(_) => path = BezPath::new(),
            TypedInstruction::StrokePath(_) | TypedInstruction::CloseAndStrokePath(_) => {
                calls.push((pen(&stack[top]), None));
                path = BezPath::new();
            }
            TypedInstruction::FillPathNonZero(_)
            | TypedInstruction::FillPathNonZeroCompatibility(_)
            | TypedInstruction::FillPathEvenOdd(_) => {
                let fill = (std::mem::take(&mut path), stack[top].ctm);
                calls.push((pen(&stack[top]), Some(fill)));
            }
            TypedInstruction::FillAndStrokeNonZero(_)
            | TypedInstruction::FillAndStrokeEvenOdd(_)
            | TypedInstruction::CloseFillAndStrokeNonZero(_)
            | TypedInstruction::CloseFillAndStrokeEvenOdd(_) => {
                if matches!(
                    op,
                    TypedInstruction::CloseFillAndStrokeNonZero(_)
                        | TypedInstruction::CloseFillAndStrokeEvenOdd(_)
                ) {
                    path.close_path();
                }
                let fill = (std::mem::take(&mut path), stack[top].ctm);
                calls.push((pen(&stack[top]), Some(fill)));
                calls.push((pen(&stack[top]), None));
            }
            _ => {}
        }
    });
    if calls.len() != device.path_calls.len() {
        return;
    }
    for (call, (pen, fill)) in device.path_calls.iter().zip(calls) {
        let Some(i) = *call else { continue };
        let drawn = &mut device.paths[i];
        drawn.pen = Some(pen);
        // The interpreter draws a fill without width or height as a thin
        // line across its box; the import reads the path that was filled.
        let Some((user, ctm)) = fill else { continue };
        if let (Some((rgb, _)), None) = (drawn.stroke, drawn.fill) {
            let (subpaths, closes) = subpaths(&user, initial * ctm);
            if !subpaths.is_empty() {
                (drawn.subpaths, drawn.closes, drawn.stroke, drawn.fill) =
                    (subpaths, closes, None, Some(rgb));
            }
        }
    }
}

/// The points of a path that is one straight loop bounding a convex area,
/// counter-clockwise; `None` for any other path.
fn convex_loop(path: &BezPath) -> Option<Vec<[f64; 2]>> {
    let mut points: Vec<[f64; 2]> = Vec::new();
    for (k, el) in path.elements().iter().enumerate() {
        match *el {
            PathEl::MoveTo(p) if k == 0 => points.push(pt(p)),
            PathEl::LineTo(p) => points.push(pt(p)),
            PathEl::ClosePath => {}
            _ => return None,
        }
    }
    points.dedup_by(|a, b| same(*a, *b));
    if points.len() > 1 && same(points[0], points[points.len() - 1]) {
        points.pop();
    }
    if points.len() < 3 {
        return None;
    }
    let n = points.len();
    let turns: Vec<f64> = (0..n)
        .map(|i| cross(points[i], points[(i + 1) % n], points[(i + 2) % n]))
        .collect();
    let area: f64 = turns.iter().sum();
    if turns.iter().any(|t| t * area < -1e-12) {
        return None;
    }
    if area < 0.0 {
        points.reverse();
    }
    Some(points)
}

fn same(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-12
}

/// Twice the signed area of the triangle a, b, c (positive counter-clockwise).
fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Whether `p` lies in the counter-clockwise convex polygon (or on it).
pub fn inside_convex(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    (0..poly.len()).all(|i| cross(poly[i], poly[(i + 1) % poly.len()], p) >= -1e-9)
}

/// A polygon cut to a counter-clockwise convex one (Sutherland–Hodgman).
pub fn clip_to_convex(subject: &[[f64; 2]], clip: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = subject.to_vec();
    for i in 0..clip.len() {
        let (a, b) = (clip[i], clip[(i + 1) % clip.len()]);
        let input = std::mem::take(&mut out);
        let side = |p: [f64; 2]| cross(a, b, p);
        for k in 0..input.len() {
            let (p, q) = (input[k], input[(k + 1) % input.len()]);
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
    }
    out
}

/// The part of the segment a–b inside a counter-clockwise convex polygon
/// (Cyrus–Beck), or `None`.
pub fn clip_segment(a: [f64; 2], b: [f64; 2], clip: &[[f64; 2]]) -> Option<([f64; 2], [f64; 2])> {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for i in 0..clip.len() {
        let (p, q) = (clip[i], clip[(i + 1) % clip.len()]);
        let (sa, sb) = (cross(p, q, a), cross(p, q, b));
        if sa < 0.0 && sb < 0.0 {
            return None;
        }
        if sa < 0.0 {
            t0 = t0.max(sa / (sa - sb));
        } else if sb < 0.0 {
            t1 = t1.min(sa / (sa - sb));
        }
    }
    let at = |t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    (t0 < t1).then(|| (at(t0), at(t1)))
}

struct PendingText {
    text: PdfText,
    font_key: u128,
    /// Baseline end, inches, for joining the next glyph.
    end: [f64; 2],
    em: f64,
    /// Optional-content sections begun before the run.
    oc: usize,
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
    oc_marks: Vec<usize>,
    /// Each path the interpreter drew, in order: the index of the path it
    /// became, `None` when left out.
    path_calls: Vec<Option<usize>>,
    /// Optional-content sections begun so far.
    oc_sections: usize,
    /// The visible area, in inches: the page, then each clip path's box
    /// within the one before it (the last is the current one).
    visible: Vec<Rect>,
    /// Each clip as a convex polygon in inches (`None` when it is not one),
    /// within the one before it.
    clips: Vec<Option<Vec<[f64; 2]>>>,
}

impl Collector {
    fn finish_text(&mut self) {
        if let Some(text) = self.current.take() {
            if !text.text.text.trim().is_empty() {
                self.order.push(Drawn::Text(self.texts.len()));
                self.oc_marks.push(text.oc);
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

/// The page transform (user space → inches) at the precision PDF import
/// reads geometry with: the current transformation matrix, in points, as
/// 16.16 fixed point held in single precision — each coefficient times
/// 65536 to the nearest integer, then to the nearest single-precision
/// number (a 0.12 scale is 7864/65536; a 345.678 offset 22654352/65536) —
/// and each user-space coordinate as a single-precision number kept to six
/// decimals (100.3 is 100.300003).
fn import_precision(transform: Affine) -> impl Fn(Point) -> Point {
    let points = (Affine::scale(72.0) * transform)
        .as_coeffs()
        .map(|v| (v * 65536.0).round() as f32 as f64 / 65536.0);
    let transform = Affine::scale(1.0 / 72.0) * Affine::new(points);
    let six = |v: f64| ((v as f32) as f64 * 1e6).round() / 1e6;
    move |p: Point| transform * Point::new(six(p.x), six(p.y))
}

fn subpaths(path: &BezPath, transform: Affine) -> (Vec<SubPath>, Vec<SubClose>) {
    let transform = import_precision(transform);
    let mut out: Vec<SubPath> = Vec::new();
    let mut closes: Vec<SubClose> = Vec::new();
    let mut start = Point::ZERO;
    let mut last = Point::ZERO;
    // The interpreter moves the end of a line onto its own start 0.0001 to
    // the right (so round caps show); such a line is drawn on one point.
    let (mut user_last, mut user_start) = (Point::ZERO, Point::ZERO);
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                (user_last, user_start) = (p, p);
                let p = transform(p);
                out.push(SubPath {
                    segments: Vec::new(),
                    closed: false,
                });
                closes.push(SubClose::default());
                start = p;
                last = p;
            }
            PathEl::LineTo(p) => {
                let p = if (p.x - user_last.x - 1e-4).abs() < 1e-9 && p.y == user_last.y {
                    user_last
                } else {
                    p
                };
                user_last = p;
                let p = transform(p);
                if let Some(sp) = out.last_mut() {
                    sp.segments.push(Segment::Line(pt(last), pt(p)));
                }
                last = p;
            }
            PathEl::QuadTo(c, p) => {
                user_last = p;
                let (c, p) = (transform(c), transform(p));
                let c1 = last + (c - last) * (2.0 / 3.0);
                let c2 = p + (c - p) * (2.0 / 3.0);
                if let Some(sp) = out.last_mut() {
                    sp.segments
                        .push(Segment::Cubic(pt(last), pt(c1), pt(c2), pt(p)));
                }
                last = p;
            }
            PathEl::CurveTo(c1, c2, p) => {
                user_last = p;
                let (c1, c2, p) = (transform(c1), transform(c2), transform(p));
                if let Some(sp) = out.last_mut() {
                    sp.segments
                        .push(Segment::Cubic(pt(last), pt(c1), pt(c2), pt(p)));
                }
                last = p;
            }
            PathEl::ClosePath => {
                user_last = user_start;
                if let (Some(sp), Some(close)) = (out.last_mut(), closes.last_mut()) {
                    close.op = true;
                    if (last - start).hypot() > 1e-9 {
                        sp.segments.push(Segment::Line(pt(last), pt(start)));
                        close.line = true;
                    }
                    sp.closed = true;
                }
                last = start;
            }
        }
    }
    let mut keep = out.iter().map(|sp| !sp.segments.is_empty());
    closes.retain(|_| keep.next().unwrap_or(false));
    out.retain(|sp| !sp.segments.is_empty());
    // A path drawn back to its start without a close operator is closed.
    for sp in &mut out {
        let (first, last) = (
            sp.segments[0].start(),
            sp.segments[sp.segments.len() - 1].end(),
        );
        if !sp.closed && (first[0] - last[0]).hypot(first[1] - last[1]) < 1e-9 {
            sp.closed = true;
        }
    }
    (out, closes)
}

impl<'a> Device<'a> for Collector {
    fn set_soft_mask(&mut self, _: Option<SoftMask<'a>>) {}
    fn set_blend_mode(&mut self, _: BlendMode) {}
    fn draw_path(
        &mut self,
        path: &BezPath,
        transform: Affine,
        paint: &Paint<'a>,
        mode: &PathDrawMode,
    ) {
        // Graphics drawn between glyphs do not end a text run: the next
        // glyph still joins it when it follows on along the baseline.
        self.path_calls.push(None);
        let (subpaths, closes) = subpaths(path, transform);
        if subpaths.is_empty() {
            return;
        }
        let points = || {
            subpaths.iter().flat_map(|sp| &sp.segments).flat_map(|seg| match seg {
                Segment::Line(a, b) => vec![*a, *b],
                Segment::Cubic(a, b, c, d) => vec![*a, *b, *c, *d],
            })
        };
        // A path reaching out of a polygon clip is clipped to it on import.
        let clip = match self.clips.last() {
            Some(Some(poly)) if points().any(|p| !inside_convex(poly, p)) => Some(poly.clone()),
            _ => None,
        };
        // A path wholly outside the visible area is left out; one that
        // reaches into it is kept whole.
        if let Some(area) = self.visible.last() {
            let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
            for seg in subpaths.iter().flat_map(|sp| &sp.segments) {
                let points: &[[f64; 2]] = match seg {
                    Segment::Line(a, b) => &[*a, *b],
                    Segment::Cubic(a, b, c, d) => &[*a, *b, *c, *d],
                };
                for p in points {
                    for k in 0..2 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
            }
            if hi[0] < area.x0 || lo[0] > area.x1 || hi[1] < area.y0 || lo[1] > area.y1 {
                return;
            }
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
        if let Some(call) = self.path_calls.last_mut() {
            *call = Some(self.paths.len());
        }
        self.order.push(Drawn::Path(self.paths.len()));
        self.oc_marks.push(self.oc_sections);
        self.paths.push(PdfPath {
            subpaths,
            stroke,
            fill,
            pen: None,
            closes,
            clip,
        });
    }
    fn push_clip_path(&mut self, clip: &ClipPath) {
        let bounds = clip.path.bounding_box();
        let area = self.visible.last().map_or(bounds, |a| a.intersect(bounds));
        self.visible.push(area);
        // A clip of one straight convex loop is kept as a polygon (within
        // the one before it); any other clip is known by its box alone.
        let polygon = convex_loop(&clip.path).and_then(|poly| match self.clips.last() {
            Some(Some(outer)) => Some(clip_to_convex(&poly, outer)),
            Some(None) => None,
            None => Some(poly),
        });
        self.clips.push(polygon);
    }
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_glyph(
        &mut self,
        glyph: &Glyph<'a>,
        transform: Affine,
        glyph_transform: Affine,
        paint: &Paint<'a>,
        mode: &GlyphDrawMode,
    ) {
        // Invisible text (render mode 3, as over a scan) is not drawn.
        if matches!(mode, GlyphDrawMode::Invisible) {
            return;
        }
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
        let origin = pt(origin);
        // A glyph continues the run on its baseline (within half an em
        // across it) when the gap along it is under an em, whatever its font
        // or size; a gap of more than a few hundredths of an em reads as a
        // space.
        let gap = self.current.as_ref().and_then(|run| {
            if (run.text.rotation - rotation).abs() >= 1e-6 {
                return None;
            }
            let (c, s) = (rotation.cos(), rotation.sin());
            let (dx, dy) = (origin[0] - run.end[0], origin[1] - run.end[1]);
            let along = dx * c + dy * s;
            let across = -dx * s + dy * c;
            (across.abs() < 0.5 * run.em && along > -0.5 * run.em && along < run.em)
                .then_some(along / run.em)
        });
        if gap.is_none() {
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
                    em,
                },
                font_key: key,
                end: origin,
                em,
                oc: self.oc_sections,
            });
        }
        if let Some(run) = self.current.as_mut() {
            if gap.is_some_and(|g| g >= 0.15) && !run.text.text.ends_with(' ') {
                run.text.text.push(' ');
            }
            run.text.text.push_str(&text);
            run.end = [origin[0] + adv_vec.x, origin[1] + adv_vec.y];
            run.text.width = ((run.end[0] - run.text.origin[0]).powi(2)
                + (run.end[1] - run.text.origin[1]).powi(2))
            .sqrt();
        }
    }
    fn begin_marked_content(&mut self, tag: &[u8], _: Option<i32>) {
        if tag == b"OC" {
            self.oc_sections += 1;
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
            self.oc_marks.push(self.oc_sections);
            self.images.push(image);
        }
    }
    fn pop_clip_path(&mut self) {
        if self.visible.len() > 1 {
            self.visible.pop();
        }
        self.clips.pop();
    }
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
        [0.0, 0.25, 0.5, 0.75]
            .iter()
            .all(|t| (dist(bezier(s, *t)) - r).abs() <= tol)
    });
    on_circle.then_some((c, r))
}

// ── Snapping ────────────────────────────────────────────────────────────────

/// PDFOSNAP, DWFOSNAP and DGNOSNAP: whether object snaps find the geometry
/// inside underlays of each kind.
static UNDERLAY_OSNAP: [AtomicBool; 3] = [
    AtomicBool::new(true),
    AtomicBool::new(true),
    AtomicBool::new(true),
];

fn osnap_slot(kind: codec::entities::UnderlayType) -> &'static AtomicBool {
    match kind {
        codec::entities::UnderlayType::Pdf => &UNDERLAY_OSNAP[0],
        codec::entities::UnderlayType::Dwf => &UNDERLAY_OSNAP[1],
        codec::entities::UnderlayType::Dgn => &UNDERLAY_OSNAP[2],
    }
}

pub fn underlay_osnap(kind: codec::entities::UnderlayType) -> bool {
    osnap_slot(kind).load(Ordering::Relaxed)
}

pub fn set_underlay_osnap(kind: codec::entities::UnderlayType, on: bool) {
    osnap_slot(kind).store(on, Ordering::Relaxed);
}

/// UOSNAP: 1 when snaps find every kind of underlay geometry, 0 when none,
/// 2 when the kinds differ.
pub fn uosnap() -> i16 {
    match UNDERLAY_OSNAP
        .iter()
        .filter(|s| s.load(Ordering::Relaxed))
        .count()
    {
        0 => 0,
        n if n == UNDERLAY_OSNAP.len() => 1,
        _ => 2,
    }
}

pub fn set_uosnap(on: bool) {
    for slot in &UNDERLAY_OSNAP {
        slot.store(on, Ordering::Relaxed);
    }
}

/// Most snap points one underlay contributes.
// ponytail: a flat cap; a spatial index over the page segments if large
// drawings need every point.
const MAX_SNAP_POINTS: usize = 20_000;

/// Endpoint and midpoint snaps of the underlay's PDF geometry, in world
/// space, when PDFOSNAP is on and the page is shown.
pub fn underlay_snap_points(
    u: &codec::entities::Underlay,
    document: &codec::Document,
) -> Vec<(glam::DVec3, crate::scene::model::wire_model::SnapHint)> {
    use crate::scene::model::wire_model::SnapHint;
    if !underlay_osnap(u.underlay_type)
        || !u.flags.contains(codec::entities::UnderlayDisplayFlags::ON)
    {
        return Vec::new();
    }
    let Some(def) = crate::entities::underlay::definition(u, document) else {
        return Vec::new();
    };
    let Some(vectors) = underlay_vectors(u, def) else {
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

/// The page's vectors in page units: PDF through its layer overrides, DWF
/// and DGN from their sheet.
fn underlay_vectors(
    u: &codec::entities::Underlay,
    def: &codec::entities::UnderlayDefinition,
) -> Option<Arc<PageVectors>> {
    let page = crate::entities::underlay::page_of(def);
    match def.underlay_type {
        codec::entities::UnderlayType::Pdf => {
            page_vectors(&super::pdf_layers::underlay_source(u, &def.file_path), page)
        }
        kind => super::underlay_vector::page_vectors(
            kind,
            &def.file_path,
            page,
            &super::pdf_layers::hidden_layers(u),
        ),
    }
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
    document: &codec::Document,
) -> Vec<SnapPiece> {
    if !underlay_osnap(u.underlay_type)
        || !u.flags.contains(codec::entities::UnderlayDisplayFlags::ON)
    {
        return Vec::new();
    }
    let Some(def) = crate::entities::underlay::definition(u, document) else {
        return Vec::new();
    };
    if def.unloaded {
        return Vec::new();
    }
    let Some(vectors) = underlay_vectors(u, def) else {
        return Vec::new();
    };
    let world = |p: [f64; 2]| crate::entities::underlay::local_to_world(u, p);
    // A circle stays one only while the placement scales both axes alike.
    let uniform = (u.x_scale.abs() - u.y_scale.abs()).abs() <= 1e-9 * u.x_scale.abs().max(1e-12);
    let flatten = |seg: &Segment, points: &mut Vec<[f64; 3]>| {
        let steps = if matches!(seg, Segment::Cubic(..)) {
            12
        } else {
            1
        };
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
                out.push(SnapPiece {
                    points,
                    circle: Some((world(c), r * u.x_scale.abs())),
                });
                continue;
            }
            for seg in &sp.segments {
                let mut points = vec![world(seg.start())];
                flatten(seg, &mut points);
                total += points.len();
                out.push(SnapPiece {
                    points,
                    circle: None,
                });
            }
        }
    }
    out
}
