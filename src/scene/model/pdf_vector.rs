//! Vector content of a PDF page — paths and text runs in page inches (y up,
//! origin at the page's lower-left corner) — read through the PDF
//! interpreter. Drives snapping to an underlay's geometry and PDFIMPORT.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use hayro::hayro_interpret::font::{Glyph, GlyphRun};
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_interpret::{
    interpret_page, BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image,
    ImageDrawProps, InterpreterCache, InterpreterSettings, Paint, SoftMask,
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
    let mut groups: Vec<PageVectors> = (0..=visible.len())
        .map(|_| PageVectors::default())
        .collect();
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
fn merge_fill_and_stroke(page: &hayro::hayro_syntax::page::Page<'_>, device: &mut Collector) {
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
    let page = pdf
        .pages()
        .get(page.trim().parse::<usize>().ok()?.checked_sub(1)?)?;
    let group_name = |dict: &Dict<'_>| {
        dict.get::<hayro::hayro_syntax::object::String<'_>>(b"Name")
            .map(|s| super::pdf_layers::text_string(s.as_bytes()))
    };
    let mut out = Vec::new();
    walk_operators(page, &mut |op, resources| {
        let TypedInstruction::BeginMarkedContentWithProperties(bdc) = op else {
            return;
        };
        if bdc.0.as_ref() != b"OC" {
            return;
        }
        // A form's own properties, else the page's.
        let properties = match bdc.1.clone().into_name() {
            Some(n) if resources.properties.contains_key(n.as_ref()) => &resources.properties,
            _ => &page.resources().properties,
        };
        let name = bdc.1.clone().into_name();
        let indirect = name
            .as_ref()
            .and_then(|n| properties.get_ref(n.as_ref()))
            .is_some();
        let dict = name.and_then(|n| properties.get::<Dict<'_>>(n.as_ref()));
        let layer = dict.and_then(|dict| {
            if dict
                .get::<Name<'_>>(b"Type")
                .is_some_and(|t| t.as_ref() == b"OCMD")
            {
                let groups: Vec<Dict<'_>> = match dict.get::<Array<'_>>(b"OCGs") {
                    Some(array) => array
                        .raw_iter()
                        .filter_map(|item| item.as_obj_ref())
                        .filter_map(|r| xref.get::<Dict<'_>>(ObjectIdentifier::from(r)))
                        .collect(),
                    None => dict.get::<Dict<'_>>(b"OCGs").into_iter().collect(),
                };
                let group = if indirect {
                    groups.last()
                } else {
                    groups.first()
                };
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
            let TypedInstruction::XObject(x) = &op else {
                continue;
            };
            // Nested forms are followed a few levels deep.
            if depth >= 8 {
                continue;
            }
            let Some(stream) = resources.get_x_object(x.0) else {
                continue;
            };
            if stream
                .dict()
                .get::<Name<'_>>(b"Subtype")
                .is_none_or(|t| t.as_ref() != b"Form")
            {
                continue;
            }
            let own = match stream.dict().get::<Dict<'_>>(b"Resources") {
                // A form's own resources replace the page's, as the
                // interpreter reads them.
                Some(dict) => Resources::new(dict),
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
                state.ctm *= matrix;
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
                stack[top].ctm *= m;
            }
            TypedInstruction::LineWidth(w) => stack[top].width = f(&w.0),
            TypedInstruction::SetGraphicsState(gs) => {
                let width = resources
                    .get_ext_g_state(gs.0)
                    .and_then(|d: Dict<'_>| d.get::<f64>(b"LW"));
                if let Some(width) = width {
                    stack[top].width = width;
                }
            }
            TypedInstruction::StrokeColorDeviceGray(g) => {
                (stack[top].rgb, stack[top].components) = (to_rgb(&[f(&g.0)]), 1)
            }
            TypedInstruction::StrokeColorDeviceRgb(c) => {
                (stack[top].rgb, stack[top].components) = (to_rgb(&[f(&c.0), f(&c.1), f(&c.2)]), 3)
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
            // A segment or a close with no current point (no `m` before
            // it, as in a damaged file) is left out, as the interpreter
            // leaves it.
            TypedInstruction::LineTo(_)
            | TypedInstruction::CubicTo(_)
            | TypedInstruction::CubicStartTo(_)
            | TypedInstruction::CubicEndTo(_)
            | TypedInstruction::ClosePath(_)
                if path.elements().is_empty() => {}
            TypedInstruction::LineTo(m) => {
                let p = Point::new(f(&m.0), f(&m.1));
                path.line_to(p);
                current = p;
            }
            TypedInstruction::CubicTo(c) => {
                let p = Point::new(f(&c.4), f(&c.5));
                path.curve_to(
                    Point::new(f(&c.0), f(&c.1)),
                    Point::new(f(&c.2), f(&c.3)),
                    p,
                );
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
                ) && !path.elements().is_empty()
                {
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
    fn draw_path(&mut self, path: &BezPath, props: DrawProps<'a>, mode: &DrawMode) {
        let (transform, paint) = (props.transform, &props.paint);
        // Graphics drawn between glyphs do not end a text run: the next
        // glyph still joins it when it follows on along the baseline.
        self.path_calls.push(None);
        let (subpaths, closes) = subpaths(path, transform);
        if subpaths.is_empty() {
            return;
        }
        let points = || {
            subpaths
                .iter()
                .flat_map(|sp| &sp.segments)
                .flat_map(|seg| match seg {
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
        // Width in points: the user-space width through the transform
        // (which is in inches), back to points.
        let width =
            |line_width: f32| line_width as f64 * transform.determinant().abs().sqrt() * 72.0;
        // The interpreter draws a path that is filled and stroked as a fill,
        // then a stroke (see `merge_fill_and_stroke`), and sends no paths
        // in the two other modes.
        let (stroke, fill) = match mode {
            DrawMode::Stroke(stroke) => (Some((rgb(paint), width(stroke.line_width))), None),
            DrawMode::Fill(_) | DrawMode::FillAndStroke(..) | DrawMode::Invisible => {
                (None, Some(rgb(paint)))
            }
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
    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, mode: &DrawMode) {
        // Invisible text (render mode 3, as over a scan) is not drawn.
        if matches!(mode, DrawMode::Invisible) {
            return;
        }
        for glyph in run.glyphs() {
            self.glyph(glyph, props.transform * glyph.transform(), &props.paint);
        }
    }
    fn begin_marked_content(
        &mut self,
        tag: &[u8],
        _: Option<&hayro::hayro_syntax::object::Dict<'_>>,
    ) {
        if tag == b"OC" {
            self.oc_sections += 1;
        }
    }
    fn draw_image(&mut self, image: Image<'a, '_>, props: ImageDrawProps<'a>) {
        let transform = props.transform;
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
    fn pop_clip(&mut self) {
        if self.visible.len() > 1 {
            self.visible.pop();
        }
        self.clips.pop();
    }
    fn pop_transparency_group(&mut self) {}
}

impl Collector {
    /// One glyph of a run, at `full` (its transform onto the page).
    fn glyph(&mut self, glyph: &Glyph<'_>, full: Affine, paint: &Paint<'_>) {
        let Glyph::Outline(outline) = glyph else {
            return;
        };
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
        let key = font_key(outline);
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
}

/// A key for the font of `glyph`: the content key of its font program.
///
/// hayro 0.9 keys a glyph's outline, not its font (0.8 had
/// `font_cache_key`), and gives a font's data (`font_data`) for embedded
/// TrueType and CFF fonts only. A glyph's identifier hashes its program's
/// content key first, as one `u128`, then the glyph, so the first `u128` it
/// hashes is the font's key. `a_page_with_two_standard_fonts_names_each`
/// fails if that changes.
fn font_key(glyph: &hayro::hayro_interpret::font::OutlineGlyph) -> u128 {
    use std::hash::{Hash, Hasher};
    #[derive(Default)]
    struct FirstU128(Option<u128>);
    impl Hasher for FirstU128 {
        fn finish(&self) -> u64 {
            0
        }
        fn write(&mut self, _: &[u8]) {}
        fn write_u128(&mut self, value: u128) {
            self.0.get_or_insert(value);
        }
    }
    let mut hasher = FirstU128::default();
    glyph.identifier().hash(&mut hasher);
    hasher.0.unwrap_or_default()
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

fn osnap_slot(kind: document::entities::UnderlayType) -> &'static AtomicBool {
    match kind {
        document::entities::UnderlayType::Pdf => &UNDERLAY_OSNAP[0],
        document::entities::UnderlayType::Dwf => &UNDERLAY_OSNAP[1],
        document::entities::UnderlayType::Dgn => &UNDERLAY_OSNAP[2],
    }
}

pub fn underlay_osnap(kind: document::entities::UnderlayType) -> bool {
    osnap_slot(kind).load(Ordering::Relaxed)
}

pub fn set_underlay_osnap(kind: document::entities::UnderlayType, on: bool) {
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
    u: &document::entities::Underlay,
    document: &document::Document,
) -> Vec<(glam::DVec3, crate::scene::model::wire_model::SnapHint)> {
    use crate::scene::model::wire_model::SnapHint;
    if !underlay_osnap(u.underlay_type)
        || !u
            .flags
            .contains(document::entities::UnderlayDisplayFlags::ON)
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
    u: &document::entities::Underlay,
    def: &document::entities::UnderlayDefinition,
) -> Option<Arc<PageVectors>> {
    let page = crate::entities::underlay::page_of(def);
    match def.underlay_type {
        document::entities::UnderlayType::Pdf => {
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
    u: &document::entities::Underlay,
    document: &document::Document,
) -> Vec<SnapPiece> {
    if !underlay_osnap(u.underlay_type)
        || !u
            .flags
            .contains(document::entities::UnderlayDisplayFlags::ON)
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// Written by Inkscape 1.4.4 (cairo 1.18.2); `tests/fixtures/pdf/README.md`
    /// says what it holds.
    const INKSCAPE: &[u8] = include_bytes!("../../../tests/fixtures/pdf/inkscape.pdf");

    /// A one-page PDF, 8 × 6 inches: the page's content stream and
    /// resources, entries added to the catalog, and further objects
    /// (numbered from 5).
    #[derive(Default)]
    struct Pdf<'a> {
        content: &'a str,
        resources: &'a str,
        catalog: &'a str,
        objects: &'a [String],
    }

    /// A stream object: its dictionary entries, then its data.
    fn stream(entries: &str, data: &str) -> String {
        format!(
            "<< {entries} /Length {} >>\nstream\n{data}\nendstream",
            data.len()
        )
    }

    impl Pdf<'_> {
        fn bytes(&self) -> Vec<u8> {
            let mut objects = vec![
                format!("<< /Type /Catalog /Pages 2 0 R {} >>", self.catalog),
                "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 576 432] \
                     /Resources << {} >> /Contents 4 0 R >>",
                    self.resources
                ),
                stream("", self.content),
            ];
            objects.extend(self.objects.iter().cloned());
            let mut out = b"%PDF-1.7\n".to_vec();
            let mut offsets = Vec::new();
            for (k, object) in objects.iter().enumerate() {
                offsets.push(out.len());
                out.extend(format!("{} 0 obj\n{object}\nendobj\n", k + 1).bytes());
            }
            let xref = out.len();
            out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
            for offset in offsets {
                out.extend(format!("{offset:010} 00000 n \n").bytes());
            }
            out.extend(
                format!(
                    "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                    objects.len() + 1
                )
                .bytes(),
            );
            out
        }

        fn read(&self, name: &str) -> PageVectors {
            read(name, self.bytes()).expect("a page")
        }
    }

    /// The source name the bytes are registered under for `name`.
    fn source(name: &str, bytes: Vec<u8>) -> String {
        let path = format!("pdf-vector-test-{name}.pdf");
        crate::scene::model::pdf_raster::register_source(&path, Arc::new(bytes));
        path
    }

    /// A one-page PDF, 8 × 6 inches, holding `content`.
    fn pdf(content: &str) -> Vec<u8> {
        Pdf {
            content,
            ..Pdf::default()
        }
        .bytes()
    }

    /// Page 1 of `bytes`, with its images, registered under a name of its
    /// own.
    fn read(name: &str, bytes: Vec<u8>) -> Option<PageVectors> {
        page_content(&source(name, bytes), "1")
    }

    /// The Base-14 Helvetica as `/F1`, under a subset name.
    const HELVETICA: &str = "/Font << /F1 5 0 R >>";
    fn helvetica() -> Vec<String> {
        vec!["<< /Type /Font /Subtype /Type1 /BaseFont /ABCDEF+Helvetica >>".to_string()]
    }

    fn near(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    fn assert_point(actual: [f64; 2], expected: [f64; 2], what: &str) {
        assert!(
            near(actual[0], expected[0], 1e-6) && near(actual[1], expected[1], 1e-6),
            "{what}: {actual:?}, expected {expected:?}"
        );
    }

    /// The points each subpath passes through: each segment's start, then
    /// the last end.
    fn corners(path: &PdfPath) -> Vec<Vec<[f64; 2]>> {
        path.subpaths
            .iter()
            .map(|sp| {
                let mut points: Vec<[f64; 2]> = sp.segments.iter().map(Segment::start).collect();
                points.extend(sp.segments.last().map(Segment::end));
                points
            })
            .collect()
    }

    #[test]
    fn a_segment_with_no_current_point_is_left_out() {
        // Each segment or close comes before any `m`; the line after it is
        // read with its own pen, and the line filled after that (a fill with
        // no area) as the path that was filled.
        for (k, stray) in [
            "72 72 l S",
            "h S",
            "72 72 144 144 216 72 c S",
            "144 144 216 72 v S",
            "72 72 216 72 y S",
            "72 72 l f",
            "b",
        ]
        .into_iter()
        .enumerate()
        {
            let content =
                format!("{stray} 0 0 1 RG 3 w 72 144 m 144 144 l S 0 1 0 rg 72 216 m 144 216 l f");
            let page = read(&format!("no-current-point-{k}"), pdf(&content))
                .unwrap_or_else(|| panic!("{stray:?}: no page"));
            // (The interpreter draws the fill of an empty path as a hairline
            // at the page's origin, which is read as it is drawn.)
            let expected = if stray.ends_with('f') || stray == "b" {
                3
            } else {
                2
            };
            assert_eq!(page.paths.len(), expected, "{stray:?}");
            let line = &page.paths[expected - 2];
            assert_eq!(
                line.subpaths[0].segments,
                [Segment::Line([1.0, 2.0], [2.0, 2.0])],
                "{stray:?}"
            );
            assert_eq!(line.stroke, Some(([0, 0, 255], 3.0)), "{stray:?}");
            assert_eq!(line.pen, line.stroke, "{stray:?}");
            let filled = &page.paths[expected - 1];
            assert_eq!(
                filled.subpaths[0].segments,
                [Segment::Line([1.0, 3.0], [2.0, 3.0])],
                "{stray:?}"
            );
            assert_eq!(
                (filled.fill, filled.stroke),
                (Some([0, 255, 0]), None),
                "{stray:?}"
            );
        }
    }

    #[test]
    fn lines_curves_and_closes_are_read_in_page_inches() {
        // 72 points to the inch, y up from the page's lower-left corner.
        let page = Pdf {
            content: "72 72 m 144 72 l 144 144 216 144 216 72 c S \
                      72 216 m 144 216 l 144 288 l h S \
                      216 216 72 72 re S \
                      288 216 m 360 216 l 360 288 l 288 216 l S \
                      360 72 m 432 72 l 432 144 l 360 72 l h S \
                      72 360 m 144 432 216 360 v S \
                      288 360 m 360 432 432 360 y S",
            ..Pdf::default()
        }
        .read("lines");
        assert_eq!(page.paths.len(), 7);
        assert!(page.texts.is_empty() && page.images.is_empty());

        // A line and a Bézier, open.
        let open = &page.paths[0].subpaths[0];
        assert!(!open.closed);
        assert_eq!(
            open.segments,
            [
                Segment::Line([1.0, 1.0], [2.0, 1.0]),
                Segment::Cubic([2.0, 1.0], [2.0, 2.0], [3.0, 2.0], [3.0, 1.0]),
            ]
        );
        assert_eq!(page.paths[0].closes, [SubClose::default()]);

        // Closed by `h`: the closing line is added, and marked as such.
        let closed = &page.paths[1];
        assert!(closed.subpaths[0].closed);
        assert_eq!(closed.subpaths[0].segments.len(), 3);
        assert_eq!(
            closed.subpaths[0].segments[2],
            Segment::Line([2.0, 4.0], [1.0, 3.0])
        );
        assert_eq!(
            closed.closes,
            [SubClose {
                op: true,
                line: true
            }]
        );

        // A rectangle: four sides, closed by its operator.
        let rect = &page.paths[2];
        assert_eq!(
            corners(rect),
            [vec![
                [3.0, 3.0],
                [4.0, 3.0],
                [4.0, 4.0],
                [3.0, 4.0],
                [3.0, 3.0]
            ]]
        );
        assert_eq!(
            rect.closes,
            [SubClose {
                op: true,
                line: true
            }]
        );

        // Drawn back to its start without `h`: closed, though no operator
        // closed it.
        assert!(page.paths[3].subpaths[0].closed);
        assert_eq!(page.paths[3].closes, [SubClose::default()]);

        // Drawn back to its start and then closed: no line is added.
        assert_eq!(page.paths[4].subpaths[0].segments.len(), 3);
        assert_eq!(
            page.paths[4].closes,
            [SubClose {
                op: true,
                line: false
            }]
        );

        // `v` repeats the current point as the first control point, `y`
        // the end point as the second.
        assert_eq!(
            page.paths[5].subpaths[0].segments,
            [Segment::Cubic(
                [1.0, 5.0],
                [1.0, 5.0],
                [2.0, 6.0],
                [3.0, 5.0]
            )]
        );
        assert_eq!(
            page.paths[6].subpaths[0].segments,
            [Segment::Cubic(
                [4.0, 5.0],
                [5.0, 6.0],
                [6.0, 5.0],
                [6.0, 5.0]
            )]
        );

        assert_eq!(page.order, (0..7).map(Drawn::Path).collect::<Vec<_>>());
    }

    #[test]
    fn strokes_and_fills_keep_their_colour_width_and_pen() {
        let page = Pdf {
            content: "q 2 0 0 2 0 0 cm 1.5 w 0 0 1 RG 36 36 m 72 36 l S Q \
                      0.2 G 72 144 m 144 144 l S \
                      1 0 0 RG 4 w 0 0 1 rg 72 216 72 36 re f \
                      0 1 0 rg 216 216 72 36 re B \
                      216 72 72 36 re f 216 72 72 36 re S \
                      1 w /DeviceRGB CS 1 0 0 SC 360 72 m 432 72 l S \
                      /Pattern CS /P0 SCN 360 144 m 432 144 l S",
            ..Pdf::default()
        }
        .read("pens");
        let paths = &page.paths;
        assert_eq!(paths.len(), 8);

        // The width through the transform, in points; the line where the
        // transform puts it.
        assert_eq!(paths[0].stroke, Some(([0, 0, 255], 3.0)));
        assert_eq!(paths[0].fill, None);
        assert_eq!(corners(&paths[0]), [vec![[1.0, 1.0], [2.0, 1.0]]]);
        // A grey stroke, at the default width.
        assert_eq!(paths[1].stroke, Some(([51, 51, 51], 1.0)));
        assert_eq!(paths[1].pen, paths[1].stroke);

        // A fill: no stroke, but the pen that was current when it was
        // painted.
        assert_eq!(paths[2].stroke, None);
        assert_eq!(paths[2].fill, Some([0, 0, 255]));
        assert_eq!(paths[2].pen, Some(([255, 0, 0], 4.0)));

        // Filled and stroked by one operator: one path with both.
        assert_eq!(paths[3].fill, Some([0, 255, 0]));
        assert_eq!(paths[3].stroke, Some(([255, 0, 0], 4.0)));

        // A fill, then a stroke of the same rectangle: two paths.
        assert_eq!((paths[4].fill.is_some(), paths[4].stroke), (true, None));
        assert_eq!((paths[5].fill, paths[5].stroke.is_some()), (None, true));
        assert_eq!(paths[4].subpaths, paths[5].subpaths);

        // A colour set through a named colour space; a pattern reads as
        // black.
        assert_eq!(paths[6].stroke, Some(([255, 0, 0], 1.0)));
        assert_eq!(paths[6].pen, paths[6].stroke);
        assert_eq!(paths[7].stroke, Some(([0, 0, 0], 1.0)));
        assert_eq!(paths[7].pen, paths[7].stroke);
    }

    #[test]
    fn a_clip_is_kept_with_the_paths_that_reach_out_of_it() {
        let page = Pdf {
            content: "q 0 0 144 144 re W n \
                      72 72 m 216 72 l S \
                      36 36 m 72 72 l S \
                      300 300 m 400 300 l S Q \
                      300 300 m 400 300 l S \
                      -200 -200 m -100 -100 l S \
                      q 288 0 m 288 144 l 432 144 l 432 0 l h W n 360 72 m 500 72 l S Q",
            ..Pdf::default()
        }
        .read("clips");
        // Out of the clip's box, and off the page: left out.
        assert_eq!(page.paths.len(), 4);
        // Reaching out of the clip: kept whole, with the clip.
        let reaching = &page.paths[0];
        assert_eq!(corners(reaching), [vec![[1.0, 1.0], [3.0, 1.0]]]);
        let clip = reaching.clip.as_ref().expect("a clip polygon");
        assert_eq!(clip.len(), 4);
        for corner in [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]] {
            assert!(clip
                .iter()
                .any(|p| near(p[0], corner[0], 1e-9) && near(p[1], corner[1], 1e-9)));
        }
        // Counter-clockwise, as the clip helpers want it.
        assert!(inside_convex(clip, [1.0, 1.0]));
        // Inside the clip: no clip.
        assert_eq!(page.paths[1].clip, None);
        // After the clip is restored: drawn, unclipped.
        let after = &corners(&page.paths[2])[0];
        assert_point(after[0], [300.0 / 72.0, 300.0 / 72.0], "line start");
        assert_point(after[1], [400.0 / 72.0, 300.0 / 72.0], "line end");
        assert_eq!(page.paths[2].clip, None);
        // A clip drawn clockwise is turned counter-clockwise.
        let turned = page.paths[3].clip.as_ref().expect("a clip polygon");
        assert!(inside_convex(turned, [5.0, 1.0]), "{turned:?}");
    }

    #[test]
    fn glyphs_on_a_baseline_make_one_run_and_wider_gaps_a_space() {
        let page = Pdf {
            content: "0 0 1 rg BT /F1 12 Tf 72 360 Td (Hel) Tj (lo) Tj ET \
                      BT /F1 12 Tf 72 288 Td [(Hello) -500 (World)] TJ ET \
                      BT /F1 12 Tf 72 216 Td [(A) -1500 (B)] TJ ET \
                      BT /F1 12 Tf 3 Tr 72 144 Td (Hidden) Tj ET \
                      BT /F1 24 Tf 0 Tr 0 1 -1 0 432 72 Tm (Up) Tj ET",
            resources: HELVETICA,
            objects: &helvetica(),
            ..Pdf::default()
        }
        .read("text");
        let values: Vec<&str> = page.texts.iter().map(|t| t.text.as_str()).collect();
        // Invisible text (render mode 3) is left out; a gap of 1.5 em parts
        // two runs.
        assert_eq!(values, ["Hello", "Hello World", "A", "B", "Up"]);

        let hello = &page.texts[0];
        // The subset prefix is dropped from the font's name.
        assert_eq!(hello.font, "Helvetica");
        assert_eq!(hello.color, [0, 0, 255]);
        assert_eq!(hello.rotation, 0.0);
        assert_point(hello.origin, [1.0, 5.0], "run origin");
        assert!(near(hello.em, 12.0 / 72.0, 1e-9));
        assert!(near(hello.cap_height, 0.72 * hello.em, 1e-12));
        // Helvetica's advances (H 722, e 556, l 222, l 222, o 556) at 12 pt.
        assert!(
            near(hello.width, 2278.0 * 0.012 / 72.0, 1e-6),
            "{}",
            hello.width
        );

        // The space counts the gap: Hello, 500 thousandths of an em, World
        // (W 944, o 556, r 333, l 222, d 556).
        let spaced = &page.texts[1];
        assert!(near(
            spaced.width,
            (2278.0 + 500.0 + 2611.0) * 0.012 / 72.0,
            1e-6
        ));

        let up = &page.texts[4];
        assert!(near(up.rotation, std::f64::consts::FRAC_PI_2, 1e-9));
        assert_point(up.origin, [6.0, 1.0], "rotated run origin");
        assert!(near(up.em, 24.0 / 72.0, 1e-9));

        assert_eq!(page.order, (0..5).map(Drawn::Text).collect::<Vec<_>>());
    }

    /// Text in two standard fonts (no font data in the file) is named by the
    /// font of each run, also when the first font comes back.
    #[test]
    fn a_page_with_two_standard_fonts_names_each() {
        let objects = [
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman >>".to_string(),
        ];
        let page = Pdf {
            content: "BT /F1 12 Tf 72 360 Td (One) Tj ET \
                      BT /F2 12 Tf 72 216 Td (Two) Tj ET \
                      BT /F1 12 Tf 72 72 Td (Three) Tj ET",
            resources: "/Font << /F1 5 0 R /F2 6 0 R >>",
            objects: &objects,
            ..Pdf::default()
        }
        .read("two-standard-fonts");
        let fonts: Vec<(&str, &str)> = page
            .texts
            .iter()
            .map(|t| (t.text.as_str(), t.font.as_str()))
            .collect();
        assert_eq!(
            fonts,
            [
                ("One", "Helvetica"),
                ("Two", "Times-Roman"),
                ("Three", "Helvetica")
            ]
        );
    }

    #[test]
    fn an_image_is_read_with_its_pixels_and_placement() {
        // Two grey pixels, black and white, drawn 2 × 1 inches at (1, 1).
        let page = Pdf {
            content: "q 144 0 0 72 72 72 cm /Im1 Do Q",
            resources: "/XObject << /Im1 5 0 R >>",
            objects: &[stream(
                "/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray \
                 /BitsPerComponent 8 /Filter /ASCIIHexDecode",
                "00FF>",
            )],
            ..Pdf::default()
        };
        let read = page.read("image");
        assert_eq!(read.images.len(), 1);
        let image = &read.images[0];
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.rgba, [0, 0, 0, 255, 255, 255, 255, 255]);
        assert_point(image.origin, [1.0, 1.0], "image origin");
        assert_point(image.u, [1.0, 0.0], "one pixel along");
        assert_point(image.v, [0.0, 1.0], "one pixel up");
        assert_eq!(read.order, [Drawn::Image(0)]);

        // The snapping read leaves images out.
        let path = source("image-snap", page.bytes());
        assert!(page_vectors(&path, "1").expect("a page").images.is_empty());
    }

    #[test]
    fn a_form_is_read_through_its_matrix() {
        let page = Pdf {
            content: "0 0 1 RG /Fm1 Do 72 144 m 144 144 l S",
            resources: "/XObject << /Fm1 5 0 R >>",
            objects: &[stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 288 216] /Matrix [2 0 0 2 0 0]",
                "1 0 0 RG 36 36 m 72 36 l S 0 0 36 18 re f",
            )],
            ..Pdf::default()
        }
        .read("form");
        assert_eq!(page.paths.len(), 3);
        // Doubled by the form's matrix, width and all.
        assert_eq!(corners(&page.paths[0]), [vec![[1.0, 1.0], [2.0, 1.0]]]);
        assert_eq!(page.paths[0].stroke, Some(([255, 0, 0], 2.0)));
        // The fill inside the form gets the form's pen.
        assert_eq!(page.paths[1].pen, Some(([255, 0, 0], 2.0)));
        // After the form, the page's own pen again.
        assert_eq!(page.paths[2].stroke, Some(([0, 0, 255], 1.0)));
        assert_eq!(page.paths[2].pen, page.paths[2].stroke);
    }

    /// A form's own resources hold the colour space of its pen: a pattern,
    /// so the colour after it does not change the pen.
    #[test]
    fn a_form_reads_the_colour_space_of_its_own_resources() {
        let page = Pdf {
            content: "/Fm1 Do",
            resources: "/XObject << /Fm1 5 0 R >>",
            objects: &[stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 288 216] \
                 /Resources << /ColorSpace << /CS0 /Pattern >> >>",
                "/CS0 CS 0.5 0.5 0.5 SC 0 0 36 18 re f",
            )],
            ..Pdf::default()
        }
        .read("form-resources");
        assert_eq!(page.paths.len(), 1);
        assert_eq!(page.paths[0].pen, Some(([0, 0, 0], 1.0)));
    }

    /// Two layers, "Walls" and "Doors", both on; a line outside them, then
    /// one in each.
    fn layered() -> Vec<u8> {
        layered_with(
            "72 72 m 144 72 l S \
             /OC /L1 BDC 72 144 m 144 144 l S EMC \
             /OC /L2 BDC 72 216 m 144 216 l S EMC",
        )
    }

    /// `content` on a page with the layers of [`layered`].
    fn layered_with(content: &str) -> Vec<u8> {
        Pdf {
            content,
            resources: "/Properties << /L1 5 0 R /L2 6 0 R >>",
            catalog: "/OCProperties << /OCGs [5 0 R 6 0 R] /D << /Order [5 0 R 6 0 R] /ON [5 0 R 6 0 R] >> >>",
            objects: &[
                "<< /Type /OCG /Name (Walls) >>".to_string(),
                "<< /Type /OCG /Name (Doors) >>".to_string(),
            ],
        }
        .bytes()
    }

    /// Each group of [`page_content_by_layer`]: its layer and the heights
    /// (inches) of the lines it holds.
    fn groups(path: &str, hidden: &[String]) -> Vec<(Option<String>, Vec<f64>)> {
        page_content_by_layer(path, "1", hidden)
            .expect("a page")
            .into_iter()
            .map(|(layer, content)| {
                let heights = content.paths.iter().map(|p| corners(p)[0][0][1]).collect();
                (layer, heights)
            })
            .collect()
    }

    #[test]
    fn content_is_split_by_pdf_layer() {
        let path = source("layers", layered());
        let page = page_content(&path, "1").expect("a page");
        assert_eq!(page.oc_sections, 2);
        assert_eq!(page.oc_marks, [0, 1, 2]);
        assert_eq!(
            groups(&path, &[]),
            [
                (None, vec![1.0]),
                (Some("Walls".to_string()), vec![2.0]),
                (Some("Doors".to_string()), vec![3.0]),
            ]
        );
        // A hidden layer is left out, with what it holds.
        assert_eq!(
            groups(&path, &["Doors".to_string()]),
            [(None, vec![1.0]), (Some("Walls".to_string()), vec![2.0])]
        );
        // A Walls section inside a Doors one: the line is on Walls (the
        // innermost layer), and turning either layer off removes it.
        let path = source(
            "nested-layers",
            layered_with(
                "/OC /L2 BDC 72 216 m 144 216 l S \
                 /OC /L1 BDC 72 288 m 144 288 l S EMC EMC",
            ),
        );
        let walls = |heights: Vec<f64>| (Some("Walls".to_string()), heights);
        let doors = |heights: Vec<f64>| (Some("Doors".to_string()), heights);
        assert_eq!(
            groups(&path, &[]),
            [(None, vec![]), walls(vec![4.0]), doors(vec![3.0])]
        );
        assert_eq!(
            groups(&path, &["Doors".to_string()]),
            [(None, vec![]), walls(vec![])]
        );
        assert_eq!(
            groups(&path, &["Walls".to_string()]),
            [(None, vec![]), doors(vec![3.0])]
        );

        // A page without layers is one group.
        let plain = source(
            "no-layers",
            Pdf {
                content: "72 72 m 144 72 l S",
                ..Pdf::default()
            }
            .bytes(),
        );
        assert_eq!(groups(&plain, &[]), [(None, vec![1.0])]);
    }

    /// Measured against the SVG Inkscape exported it from (`README.md`),
    /// and against poppler's reading of the file.
    #[test]
    fn an_inkscape_drawing_reads_as_drawn() {
        let page = read("inkscape", INKSCAPE.to_vec()).expect("a page");
        assert_eq!(page.paths.len(), 3);

        let polyline = &page.paths[0];
        assert_eq!(
            corners(polyline),
            [vec![[1.0, 5.0], [3.0, 5.0], [3.0, 4.0]]]
        );
        assert_eq!(polyline.stroke, Some(([204, 0, 0], 2.0)));

        // A circle, which cairo writes as four Béziers.
        let circle = &page.paths[1];
        assert_eq!(circle.subpaths.len(), 1);
        let (centre, radius) = circle_of(&circle.subpaths[0]).expect("a circle");
        assert_point(centre, [5.0, 4.5], "circle centre");
        assert!(near(radius, 0.5, 1e-6));
        assert_eq!(circle.stroke, Some(([0, 0, 204], 1.0)));

        // A rectangle filled and stroked by one operator.
        let rect = &page.paths[2];
        assert_eq!(rect.fill, Some([0, 170, 0]));
        assert_eq!(rect.stroke, Some(([0, 0, 0], 3.0)));
        assert!(rect.subpaths[0].closed);
        let points: Vec<[f64; 2]> = corners(rect)[0].clone();
        for corner in [[1.0, 3.0], [3.0, 3.0], [3.0, 2.0], [1.0, 2.0]] {
            assert!(points.contains(&corner), "{corner:?} in {points:?}");
        }

        let values: Vec<&str> = page.texts.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(values, ["Hello PDF", "Tilted"]);
        let hello = &page.texts[0];
        assert_eq!(hello.font, "ArialMT");
        assert_point(hello.origin, [4.0, 2.5], "text origin");
        assert!(near(hello.em, 24.0 / 72.0, 1e-9));
        assert_eq!((hello.rotation, hello.color), (0.0, [0, 0, 0]));
        // poppler's box for the two words: 288 to 397.36 points.
        assert!(
            near(hello.width, (397.363406 - 288.0) / 72.0, 1e-4),
            "{}",
            hello.width
        );
        let tilted = &page.texts[1];
        assert_eq!(tilted.font, "TimesNewRomanPSMT");
        assert_point(tilted.origin, [4.0, 1.5], "tilted origin");
        assert!(near(tilted.rotation, 30f64.to_radians(), 1e-6));
        assert!(near(tilted.em, 18.0 / 72.0, 1e-6));
        assert_eq!(tilted.color, [0, 0, 204]);

        // The 2 × 2 picture with its soft mask, 1 inch across.
        assert_eq!(page.images.len(), 1);
        let image = &page.images[0];
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(
            image.rgba,
            [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 128]
        );
        assert_point(image.origin, [6.0, 1.0], "image origin");
        assert_point(image.u, [0.5, 0.0], "one pixel along");
        assert_point(image.v, [0.0, 0.5], "one pixel up");
    }

    #[test]
    fn what_is_not_a_page_gives_none() {
        assert!(page_content("pdf-vector-test-no-such-file.pdf", "1").is_none());
        assert!(read("not-a-pdf", b"not a PDF at all".to_vec()).is_none());
        let path = source(
            "one-page",
            Pdf {
                content: "72 72 m 144 72 l S",
                ..Pdf::default()
            }
            .bytes(),
        );
        for page in ["0", "2", "one", ""] {
            assert!(page_content(&path, page).is_none(), "page {page:?}");
        }
        // Page names may carry spaces.
        assert!(page_content(&path, " 1 ").is_some());
    }

    #[test]
    fn segment_and_polygon_helpers() {
        let line = Segment::Line([0.0, 0.0], [2.0, 4.0]);
        assert_eq!(bezier(&line, 0.25), [0.5, 1.0]);
        let cubic = Segment::Cubic([0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]);
        assert_eq!(bezier(&cubic, 0.0), [0.0, 0.0]);
        assert_eq!(bezier(&cubic, 1.0), [1.0, 0.0]);
        assert_eq!(bezier(&cubic, 0.5), [0.5, 0.75]);

        // A square of lines is no circle; nor is an open loop.
        let square = SubPath {
            segments: vec![
                Segment::Line([0.0, 0.0], [1.0, 0.0]),
                Segment::Line([1.0, 0.0], [1.0, 1.0]),
                Segment::Line([1.0, 1.0], [0.0, 1.0]),
                Segment::Line([0.0, 1.0], [0.0, 0.0]),
            ],
            closed: true,
        };
        assert_eq!(circle_of(&square), None);

        let unit = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        assert!(inside_convex(&unit, [0.5, 0.5]));
        assert!(inside_convex(&unit, [1.0, 0.5]));
        assert!(!inside_convex(&unit, [1.5, 0.5]));
        assert_eq!(
            clip_segment([-1.0, 0.5], [2.0, 0.5], &unit),
            Some(([0.0, 0.5], [1.0, 0.5]))
        );
        assert_eq!(clip_segment([2.0, 0.0], [3.0, 1.0], &unit), None);
        // A square moved half a side: the quarter they share.
        let moved = unit.map(|[x, y]| [x + 0.5, y + 0.5]);
        let shared = clip_to_convex(&moved, &unit);
        for corner in [[0.5, 0.5], [1.0, 0.5], [1.0, 1.0], [0.5, 1.0]] {
            assert!(shared.contains(&corner), "{corner:?} in {shared:?}");
        }
        // A polygon clipped to itself keeps its corners, which lie on the
        // clip's edges.
        assert_eq!(clip_to_convex(&unit, &unit), unit);
        assert!(shared
            .iter()
            .all(|p| inside_convex(&unit, *p) && inside_convex(&moved, *p)));
    }

    /// Damaged copies of the hand-made pages (every truncation, and every
    /// byte set to 0, to 0xFF and with its top bit flipped) and of the
    /// Inkscape file (a spread of each) read without a panic.
    #[test]
    fn damaged_files_never_panic() {
        let text = Pdf {
            content:
                "1 0 0 RG 2 w 0 0 1 rg 72 72 72 36 re B q 0 0 144 144 re W n 72 72 m 216 72 l S Q \
                      BT /F1 12 Tf 72 288 Td [(Hello) -500 (World)] TJ ET /Fm1 Do",
            resources: "/Font << /F1 5 0 R >> /XObject << /Fm1 6 0 R >>",
            objects: &[
                helvetica()[0].clone(),
                stream(
                    "/Type /XObject /Subtype /Form /BBox [0 0 288 216] /Matrix [2 0 0 2 0 0]",
                    "36 36 m 72 36 l S",
                ),
            ],
            ..Pdf::default()
        }
        .bytes();
        let mut cases = 0;
        let mut check = |bytes: &[u8]| {
            cases += 1;
            let path = source("damaged", bytes.to_vec());
            let _ = page_content(&path, "1");
            let _ = page_content_by_layer(&path, "1", &["Doors".to_string()]);
        };
        let layered = layered();
        for (file, step) in [(&text[..], 1), (&layered[..], 1), (INKSCAPE, 37)] {
            for end in (0..file.len()).step_by(step) {
                check(&file[..end]);
            }
            let mut damaged = file.to_vec();
            for at in (0..file.len()).step_by(step) {
                for value in [0x00, 0xFF, file[at] ^ 0x80] {
                    damaged[at] = value;
                    check(&damaged);
                }
                damaged[at] = file[at];
            }
        }
        assert!(cases > 5_000, "{cases} damaged files");
    }
}
