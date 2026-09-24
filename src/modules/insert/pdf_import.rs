// PDFIMPORT — turn the vector content of a PDF underlay into drawing objects.
//
//   Select PDF underlay or [File] <File>:
//   Specify first corner of area to import or [Polygonal/All/Settings] <All>:
//   Specify opposite corner:
//   Keep, Detach or Unload PDF underlay? [Keep/Detach/Unload] <Unload>:
//   Binding PDF file <path>, page <n> ...
//
// Content of a PDF layer goes to PDF_<layer>; the rest to PDF_Geometry,
// PDF_Solid Fills, PDF_Text and PDF_Images (or those alone, or the current
// layer, per the settings). Straight strokes that meet become polylines, a
// closed four-arc Bézier loop that is a circle a CIRCLE, other curves
// splines, filled triangles and quadrilaterals SOLIDs at 50% transparency
// (other fills solid hatches), text runs MTEXT in a "PDF <font>" style and
// raster images PNG files referenced by IMAGE objects.

use acadrust::entities::{
    AttachmentPoint, BoundaryEdge, BoundaryPath, Circle, Hatch, LwPolyline, MText, PolylineEdge,
    Solid, Spline, Underlay, UnderlayType,
};
use acadrust::types::{Color, Handle, LineWeight, Vector2, Vector3};
use acadrust::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::model::pdf_vector::{
    bezier, circle_of as as_circle, PageVectors, PdfPath, Segment, SubPath,
};
use crate::scene::model::wire_model::WireModel;

/// Which layers imported objects go to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportLayers {
    /// The PDF's own layers ("PDF_<name>"); content outside them to
    /// PDF_Geometry / PDF_Text.
    Pdf,
    /// One layer per kind of object.
    Object,
    /// The current layer.
    Current,
}

/// The PDF Import Settings, kept for the session. The options below the
/// data section are PDFIMPORTMODE's bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdfImportSettings {
    pub vector: bool,
    pub fills: bool,
    pub text: bool,
    pub raster: bool,
    pub layers: ImportLayers,
    pub as_block: bool,
    pub join: bool,
    pub hatches: bool,
    pub lineweights: bool,
    pub linetypes: bool,
}

impl Default for PdfImportSettings {
    fn default() -> Self {
        Self {
            vector: true,
            fills: true,
            text: true,
            raster: false,
            layers: ImportLayers::Pdf,
            as_block: false,
            join: true,
            hatches: false,
            lineweights: true,
            linetypes: false,
        }
    }
}

impl PdfImportSettings {
    /// PDFIMPORTMODE: 1 import as block, 2 apply lineweights, 4 join
    /// segments, 8 fills as hatches, 16 infer linetypes (default 6).
    pub fn mode(&self) -> i16 {
        i16::from(self.as_block)
            | i16::from(self.lineweights) << 1
            | i16::from(self.join) << 2
            | i16::from(self.hatches) << 3
            | i16::from(self.linetypes) << 4
    }

    /// PDFIMPORTFILTER: the kinds left out — 1 vector geometry, 2 text,
    /// 4 solid fills, 8 raster images (default 8).
    pub fn filter(&self) -> i16 {
        i16::from(!self.vector)
            | i16::from(!self.text) << 1
            | i16::from(!self.fills) << 2
            | i16::from(!self.raster) << 3
    }

    pub fn set_filter(&mut self, filter: i16) {
        self.vector = filter & 1 == 0;
        self.text = filter & 2 == 0;
        self.fills = filter & 4 == 0;
        self.raster = filter & 8 == 0;
    }

    /// PDFIMPORTLAYERS: 0 PDF layers, 1 object layers, 2 current layer.
    pub fn layers_value(&self) -> i16 {
        match self.layers {
            ImportLayers::Pdf => 0,
            ImportLayers::Object => 1,
            ImportLayers::Current => 2,
        }
    }

    pub fn set_mode(&mut self, mode: i16) {
        self.as_block = mode & 1 != 0;
        self.lineweights = mode & 2 != 0;
        self.join = mode & 4 != 0;
        self.hatches = mode & 8 != 0;
        self.linetypes = mode & 16 != 0;
    }
}

static SETTINGS: std::sync::Mutex<Option<PdfImportSettings>> = std::sync::Mutex::new(None);

pub fn import_settings() -> PdfImportSettings {
    SETTINGS
        .lock()
        .map(|s| s.unwrap_or_default())
        .unwrap_or_default()
}

pub fn set_import_settings(settings: PdfImportSettings) {
    if let Ok(mut s) = SETTINGS.lock() {
        *s = Some(settings);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImportArea {
    All,
    Polygon(Vec<DVec3>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnderlayMode {
    Keep,
    Detach,
    Unload,
}

#[derive(Clone, Debug)]
pub struct PdfImportRequest {
    pub underlay: Handle,
    pub area: ImportArea,
    pub mode: UnderlayMode,
}

/// A page of a file imported without an underlay (the File option): placed
/// at `insertion` with a scale and rotation.
#[derive(Clone, Debug)]
pub struct PdfFileImport {
    pub path: String,
    pub page: String,
    pub scale: f64,
    pub rotation: f64,
    pub insertion: DVec3,
}

impl PdfFileImport {
    /// The underlay the page would be as an attachment, for placing it.
    pub fn placement(&self) -> Underlay {
        let mut u = Underlay::pdf();
        u.insertion_point = Vector3::new(self.insertion.x, self.insertion.y, self.insertion.z);
        u.set_scale(self.scale);
        u.rotation = self.rotation;
        u
    }
}

/// Import PDF with the insertion point asked on screen.
pub struct PdfImportPointCommand {
    import: PdfFileImport,
}

impl PdfImportPointCommand {
    pub fn new(import: PdfFileImport) -> Self {
        Self { import }
    }
}

impl CadCommand for PdfImportPointCommand {
    fn name(&self) -> &'static str {
        "PDFIMPORT"
    }
    fn prompt(&self) -> String {
        "Specify insertion point:".to_string()
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.import.insertion = pt;
        CmdResult::PdfImportFile(self.import.clone())
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Select,
    Area,
    Opposite,
    Polygon,
    Mode,
}

pub struct PdfImportCommand {
    step: Step,
    handle: Handle,
    picked: Option<EntityType>,
    points: Vec<DVec3>,
    area: ImportArea,
}

impl PdfImportCommand {
    /// Import from an underlay already chosen (the underlay tab's button):
    /// starts at the area prompt.
    pub fn for_underlay(handle: Handle) -> Self {
        Self {
            step: Step::Area,
            handle,
            ..Self::new()
        }
    }

    pub fn new() -> Self {
        Self {
            step: Step::Select,
            handle: Handle::NULL,
            picked: None,
            points: Vec::new(),
            area: ImportArea::All,
        }
    }

    fn area_choice(&mut self, text: &str) -> CmdResult {
        match text.trim().to_ascii_uppercase().as_str() {
            "" | "A" | "ALL" => {
                self.area = ImportArea::All;
                self.step = Step::Mode;
                CmdResult::NeedPoint
            }
            "P" | "POLYGONAL" => {
                self.points.clear();
                self.step = Step::Polygon;
                CmdResult::NeedPoint
            }
            "S" | "SETTINGS" => CmdResult::OpenPdfImportSettings,
            _ => CmdResult::ReportError("Invalid option keyword.".to_string()),
        }
    }

    fn mode_choice(&mut self, text: &str) -> CmdResult {
        let mode = match text.trim().to_ascii_uppercase().as_str() {
            "" | "U" | "UNLOAD" => UnderlayMode::Unload,
            "K" | "KEEP" => UnderlayMode::Keep,
            "D" | "DETACH" => UnderlayMode::Detach,
            _ => return CmdResult::ReportError("Invalid option keyword.".to_string()),
        };
        CmdResult::PdfImport(PdfImportRequest {
            underlay: self.handle,
            area: self.area.clone(),
            mode,
        })
    }
}

impl CadCommand for PdfImportCommand {
    fn name(&self) -> &'static str {
        "PDFIMPORT"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Select => "Select PDF underlay or [File] <File>:".to_string(),
            Step::Area => {
                "Specify first corner of area to import or [Polygonal/All/Settings] <All>:".to_string()
            }
            Step::Opposite => "Specify opposite corner:".to_string(),
            Step::Polygon if self.points.is_empty() => "Specify first point:".to_string(),
            Step::Polygon => "Specify next point or [Undo]:".to_string(),
            Step::Mode => {
                "Keep, Detach or Unload PDF underlay? [Keep/Detach/Unload] <Unload>:".to_string()
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Select => vec![CmdOption::new("File", "F")],
            Step::Area => vec![
                CmdOption::new("Polygonal", "P"),
                CmdOption::new("All", "A"),
                CmdOption::new("Settings", "S"),
            ],
            Step::Polygon if !self.points.is_empty() => vec![CmdOption::new("Undo", "U")],
            Step::Mode => vec![
                CmdOption::new("Keep", "K"),
                CmdOption::new("Detach", "D"),
                CmdOption::new("Unload", "U"),
            ],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Mode => InputKind::SingleToken,
            _ => InputKind::Point,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::Select | Step::Area | Step::Polygon)
    }

    fn needs_entity_pick(&self) -> bool {
        self.step == Step::Select
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        match self.picked.take() {
            Some(EntityType::Underlay(u)) if u.underlay_type == UnderlayType::Pdf => {
                self.handle = handle;
                self.step = Step::Area;
                CmdResult::NeedPoint
            }
            _ => CmdResult::ReportError("Object selected was not a PDF underlay.".to_string()),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::Area => {
                self.points = vec![pt];
                self.step = Step::Opposite;
                CmdResult::NeedPoint
            }
            Step::Opposite => {
                let a = self.points[0];
                self.area = ImportArea::Polygon(vec![
                    a,
                    DVec3::new(pt.x, a.y, a.z),
                    pt,
                    DVec3::new(a.x, pt.y, a.z),
                ]);
                self.step = Step::Mode;
                CmdResult::NeedPoint
            }
            Step::Polygon => {
                self.points.push(pt);
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let token = text.trim().to_ascii_uppercase();
        Some(match self.step {
            Step::Select if token == "F" || token == "FILE" => {
                CmdResult::Dispatch("_PDFIMPORTFILE".to_string())
            }
            Step::Area => self.area_choice(&token),
            Step::Polygon if token == "U" || token == "UNDO" => {
                self.points.pop();
                CmdResult::NeedPoint
            }
            Step::Mode => self.mode_choice(&token),
            _ => return None,
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Select => CmdResult::Dispatch("_PDFIMPORTFILE".to_string()),
            Step::Area => self.area_choice(""),
            Step::Polygon if self.points.len() >= 3 => {
                self.area = ImportArea::Polygon(self.points.clone());
                self.step = Step::Mode;
                CmdResult::NeedPoint
            }
            Step::Polygon => CmdResult::NeedPoint,
            Step::Mode => self.mode_choice(""),
            Step::Opposite => CmdResult::NeedPoint,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let ring: Vec<DVec3> = match self.step {
            Step::Opposite => {
                let a = self.points[0];
                vec![a, DVec3::new(pt.x, a.y, a.z), pt, DVec3::new(a.x, pt.y, a.z)]
            }
            Step::Polygon if !self.points.is_empty() => {
                let mut ring = self.points.clone();
                ring.push(pt);
                ring
            }
            _ => return None,
        };
        let mut points: Vec<[f32; 3]> =
            ring.iter().map(|p| [p.x as f32, p.y as f32, p.z as f32]).collect();
        points.push(points[0]);
        Some(WireModel::solid("pdf_import_area".into(), points, WireModel::CYAN, false))
    }
}

// ── Conversion ───────────────────────────────────────────────────────────────

/// What an import adds: objects, and the layers and text styles they use
/// (name, colour / font file), created when missing.
#[derive(Default)]
pub struct ImportResult {
    pub entities: Vec<EntityType>,
    pub layers: Vec<(String, Color)>,
    pub text_styles: Vec<(String, String)>,
    pub images: Vec<ImageImport>,
    /// Some line uses the dash linetype.
    pub dashed: bool,
}

/// A PDF colour as an index colour when it is one of the basic seven,
/// otherwise as a true colour.
fn color_of(rgb: [u8; 3]) -> Color {
    match rgb {
        [255, 0, 0] => Color::Index(1),
        [255, 255, 0] => Color::Index(2),
        [0, 255, 0] => Color::Index(3),
        [0, 255, 255] => Color::Index(4),
        [0, 0, 255] => Color::Index(5),
        [255, 0, 255] => Color::Index(6),
        [0, 0, 0] | [255, 255, 255] => Color::Index(7),
        [r, g, b] => Color::Rgb { r, g, b },
    }
}

/// Stroke width in points → the nearest standard lineweight.
fn lineweight_of(width_pt: f64) -> LineWeight {
    const STANDARD: [i16; 24] = [
        0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158,
        200, 211,
    ];
    if width_pt <= 0.0 {
        return LineWeight::ByLayer;
    }
    let hundredths = width_pt * 25.4 / 72.0 * 100.0;
    let best = STANDARD
        .iter()
        .copied()
        .min_by(|a, b| {
            (*a as f64 - hundredths)
                .abs()
                .total_cmp(&(*b as f64 - hundredths).abs())
        })
        .unwrap_or(0);
    LineWeight::Value(best)
}

fn font_file_for(font: &str) -> String {
    let f = font.to_ascii_lowercase();
    if f.contains("times") {
        "times.ttf"
    } else if f.contains("courier") {
        "cour.ttf"
    } else {
        "arial.ttf"
    }
    .to_string()
}

fn inside(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut odd = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            odd = !odd;
        }
        j = i;
    }
    odd
}

/// Whether a path reaches into the area: any sampled point inside it.
fn path_in_area(path: &PdfPath, area: Option<&[[f64; 2]]>) -> bool {
    let Some(area) = area else { return true };
    path.subpaths.iter().any(|sp| {
        sp.segments
            .iter()
            .any(|s| [0.0, 0.5, 1.0].iter().any(|t| inside(area, bezier(s, *t))))
    })
}

/// Where imported objects of each kind go.
pub struct LayerNaming {
    pub settings: PdfImportSettings,
    /// "PDF_", or "PDF2_", "PDF3_" … for the next files imported.
    pub prefix: String,
    /// The current layer, for [`ImportLayers::Current`].
    pub current: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Geometry,
    Fill,
    Text,
    Image,
}

impl LayerNaming {
    fn layer(&self, pdf_layer: Option<&str>, kind: Kind) -> String {
        let object = |kind| match kind {
            Kind::Geometry => "Geometry",
            Kind::Fill => "Solid Fills",
            Kind::Text => "Text",
            Kind::Image => "Images",
        };
        match (self.settings.layers, pdf_layer) {
            (ImportLayers::Current, _) => self.current.clone(),
            (ImportLayers::Pdf, Some(name)) => format!("{}{name}", self.prefix),
            _ => format!("{}{}", self.prefix, object(kind)),
        }
    }
}

/// An image the import places: its pixels go to a PNG file named by the
/// caller, then a raster image at `insertion` spanning `u` × `v` per pixel.
pub struct ImageImport {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub insertion: Vector3,
    pub u: Vector3,
    pub v: Vector3,
    pub layer: String,
}

/// The linetype collinear dashes become ("Infer linetypes"): dash 0.8,
/// space 0.2, scaled per object to the dash length.
pub const DASH_LINETYPE: &str = "PDF_IMPORT";

/// A stroke before it becomes an object: straight chains can still be joined
/// or read as dashes.
struct Stroke {
    layer: String,
    color: Color,
    weight: LineWeight,
    points: Vec<[f64; 2]>,
    closed: bool,
}

fn same_point(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-7 && (a[1] - b[1]).abs() < 1e-7
}

/// Open straight strokes of one layer, colour and weight that meet end to
/// end become one polyline (closed when the chain returns to its start).
fn join_strokes(strokes: Vec<Stroke>) -> Vec<Stroke> {
    let mut out: Vec<Stroke> = Vec::new();
    let mut open: Vec<Stroke> = Vec::new();
    for s in strokes {
        if s.closed {
            out.push(s);
        } else {
            open.push(s);
        }
    }
    // Drawing order is kept: dashes read as a run only in the order drawn.
    while !open.is_empty() {
        let mut chain = open.remove(0);
        loop {
            let end = *chain.points.last().expect("strokes have points");
            let start = chain.points[0];
            let fits = |o: &Stroke| {
                o.layer == chain.layer && o.color == chain.color && o.weight == chain.weight
            };
            if let Some(i) = open.iter().position(|o| {
                fits(o) && (same_point(o.points[0], end) || same_point(*o.points.last().unwrap(), end))
            }) {
                let mut next = open.remove(i);
                if !same_point(next.points[0], end) {
                    next.points.reverse();
                }
                chain.points.extend(next.points.into_iter().skip(1));
            } else if let Some(i) = open.iter().position(|o| {
                fits(o)
                    && (same_point(*o.points.last().unwrap(), start) || same_point(o.points[0], start))
            }) {
                let mut prev = open.remove(i);
                if !same_point(*prev.points.last().unwrap(), start) {
                    prev.points.reverse();
                }
                prev.points.pop();
                prev.points.extend(chain.points);
                chain.points = prev.points;
            } else {
                break;
            }
        }
        if chain.points.len() > 3 && same_point(chain.points[0], *chain.points.last().unwrap()) {
            chain.points.pop();
            chain.closed = true;
        }
        out.push(chain);
    }
    out
}

/// Runs of three or more equal collinear dashes with equal gaps become one
/// line from the first dash's start to the last's end, with the dash length
/// (the linetype scale).
// ponytail: dashes are matched along one direction at a time in drawing
// order; patterns of mixed dash lengths stay separate lines.
fn infer_dashes(strokes: Vec<Stroke>) -> Vec<(Stroke, Option<f64>)> {
    let is_dash = |s: &Stroke| !s.closed && s.points.len() == 2;
    let len = |s: &Stroke| {
        let (a, b) = (s.points[0], s.points[1]);
        (b[0] - a[0]).hypot(b[1] - a[1])
    };
    let mut out: Vec<(Stroke, Option<f64>)> = Vec::new();
    let mut run: Vec<Stroke> = Vec::new();
    let flush = |run: &mut Vec<Stroke>, out: &mut Vec<(Stroke, Option<f64>)>| {
        if run.len() >= 3 {
            let dash = len(&run[0]);
            let mut line = run.remove(0);
            let last = run.pop().expect("three or more");
            line.points[1] = last.points[1];
            run.clear();
            out.push((line, Some(dash)));
        } else {
            out.extend(run.drain(..).map(|s| (s, None)));
        }
    };
    for s in strokes {
        if !is_dash(&s) {
            flush(&mut run, &mut out);
            out.push((s, None));
            continue;
        }
        let continues = run.last().is_some_and(|prev| {
            let first = &run[0];
            let (a, b) = (prev.points[0], prev.points[1]);
            let dir = [(b[0] - a[0]) / len(prev), (b[1] - a[1]) / len(prev)];
            let (c, d) = (s.points[0], s.points[1]);
            let gap = [c[0] - b[0], c[1] - b[1]];
            let along = gap[0] * dir[0] + gap[1] * dir[1];
            let across = (gap[0] * dir[1] - gap[1] * dir[0]).abs();
            let seg = [(d[0] - c[0]) / len(&s), (d[1] - c[1]) / len(&s)];
            let parallel = (seg[0] * dir[1] - seg[1] * dir[0]).abs() < 1e-6
                && seg[0] * dir[0] + seg[1] * dir[1] > 0.0;
            let first_gap = run.get(1).map(|second| {
                let (e, f) = (first.points[1], second.points[0]);
                (f[0] - e[0]).hypot(f[1] - e[1])
            });
            s.layer == prev.layer
                && s.color == prev.color
                && s.weight == prev.weight
                && parallel
                && across < 1e-6
                && along > 1e-9
                && (len(&s) - len(first)).abs() < 1e-6
                && first_gap.is_none_or(|g| (g - along).abs() < 1e-6)
        });
        if !continues {
            flush(&mut run, &mut out);
        }
        run.push(s);
    }
    flush(&mut run, &mut out);
    out
}

/// The objects an import creates from a page's content split by PDF layer
/// (see `page_content_by_layer`), placed as `underlay` places the page.
pub fn convert(
    content: &[(Option<String>, PageVectors)],
    underlay: &Underlay,
    area: &ImportArea,
    naming: &LayerNaming,
) -> ImportResult {
    let settings = &naming.settings;
    let world = |p: [f64; 2]| crate::entities::underlay::local_to_world(underlay, p);
    let world2 = |p: [f64; 2]| {
        let w = world(p);
        Vector2::new(w[0], w[1])
    };
    let world3 = |p: [f64; 2]| {
        let w = world(p);
        Vector3::new(w[0], w[1], w[2])
    };
    let z = underlay.insertion_point.z;
    let scale = underlay.x_scale.abs();
    // The area is tested in page space, where the page content lives.
    let area_local: Option<Vec<[f64; 2]>> = match area {
        ImportArea::All => None,
        ImportArea::Polygon(points) => Some(
            points
                .iter()
                .map(|p| {
                    let (c, s) = (underlay.rotation.cos(), underlay.rotation.sin());
                    let dx = p.x - underlay.insertion_point.x;
                    let dy = p.y - underlay.insertion_point.y;
                    [
                        (dx * c + dy * s) / underlay.x_scale,
                        (-dx * s + dy * c) / underlay.y_scale,
                    ]
                })
                .collect(),
        ),
    };
    let in_area = |p: [f64; 2]| area_local.as_deref().is_none_or(|a| inside(a, p));

    let mut result = ImportResult::default();
    let mut layer_colors: Vec<(String, Color)> = Vec::new();
    // A layer takes the colour of its first object; objects of that colour
    // then follow the layer.
    let mut color_on = |layer: &str, color: Color| -> Color {
        let layer_color = match layer_colors.iter().find(|(n, _)| n == layer) {
            Some((_, c)) => *c,
            None => {
                layer_colors.push((layer.to_string(), color));
                color
            }
        };
        if color == layer_color {
            Color::ByLayer
        } else {
            color
        }
    };
    let fill_transparency = acadrust::types::Transparency::from_alpha_value(0x0200_007F);
    let mut strokes: Vec<Stroke> = Vec::new();
    let mut curves: Vec<EntityType> = Vec::new();

    for (pdf_layer, page) in content {
        let pdf_layer = pdf_layer.as_deref();
        if settings.vector {
            for path in &page.paths {
                if !path_in_area(path, area_local.as_deref()) {
                    continue;
                }
                match (path.stroke, path.fill) {
                    (Some((rgb, width)), _) => {
                        let layer = naming.layer(pdf_layer, Kind::Geometry);
                        let color = color_on(&layer, color_of(rgb));
                        let weight = if settings.lineweights {
                            lineweight_of(width)
                        } else {
                            LineWeight::ByLayer
                        };
                        for sp in &path.subpaths {
                            if let Some((c, r)) = as_circle(sp) {
                                let w = world(c);
                                let mut circle = EntityType::Circle(Circle::from_center_radius(
                                    Vector3::new(w[0], w[1], z),
                                    r * scale,
                                ));
                                let common = circle.common_mut();
                                (common.layer, common.color, common.line_weight) =
                                    (layer.clone(), color, weight);
                                curves.push(circle);
                            } else if sp.segments.iter().all(|s| matches!(s, Segment::Line(..))) {
                                let mut points = vec![sp.segments[0].start()];
                                points.extend(sp.segments.iter().map(|s| s.end()));
                                if sp.closed && points.len() > 2 {
                                    points.pop();
                                }
                                strokes.push(Stroke {
                                    layer: layer.clone(),
                                    color,
                                    weight,
                                    points,
                                    closed: sp.closed,
                                });
                            } else {
                                let mut spline = EntityType::Spline(spline_of(sp, &world3));
                                let common = spline.common_mut();
                                (common.layer, common.color, common.line_weight) =
                                    (layer.clone(), color, weight);
                                curves.push(spline);
                            }
                        }
                    }
                    (None, Some(rgb)) if settings.fills => {
                        let layer = naming.layer(pdf_layer, Kind::Fill);
                        let color = color_on(&layer, color_of(rgb));
                        for mut entity in fill_entities(path, settings.hatches, &world2, &world3, z) {
                            let common = entity.common_mut();
                            (common.layer, common.color, common.transparency) =
                                (layer.clone(), color, fill_transparency);
                            result.entities.push(entity);
                        }
                    }
                    _ => {}
                }
            }
        }

        if settings.text {
            for text in &page.texts {
                let cap = text.cap_height.max(0.0);
                let probe = [text.origin[0] + text.width / 2.0, text.origin[1] + cap / 2.0];
                if !in_area(probe) && !in_area(text.origin) {
                    continue;
                }
                let style = format!("PDF {}", text.font);
                if !result.text_styles.iter().any(|(name, _)| *name == style) {
                    result.text_styles.push((style.clone(), font_file_for(&text.font)));
                }
                let (dx, dy) = (-text.rotation.sin(), text.rotation.cos());
                let mid = [text.origin[0] + dx * cap / 2.0, text.origin[1] + dy * cap / 2.0];
                let mut mtext = MText::new();
                mtext.value = text.text.trim_end().to_string();
                mtext.insertion_point = world3(mid);
                mtext.height = cap * scale;
                mtext.rectangle_width = 0.0;
                mtext.rotation = text.rotation + underlay.rotation;
                mtext.style = style;
                mtext.attachment_point = AttachmentPoint::MiddleLeft;
                let layer = naming.layer(pdf_layer, Kind::Text);
                mtext.common.color = color_on(&layer, color_of(text.color));
                mtext.common.layer = layer;
                result.entities.push(EntityType::MText(mtext));
            }
        }

        if settings.raster {
            let (c, s) = (underlay.rotation.cos(), underlay.rotation.sin());
            let turn = |d: [f64; 2]| {
                Vector3::new(
                    (d[0] * c - d[1] * s) * underlay.x_scale,
                    (d[0] * s + d[1] * c) * underlay.y_scale,
                    0.0,
                )
            };
            for image in &page.images {
                let mid = [
                    image.origin[0] + (image.u[0] * image.width as f64 + image.v[0] * image.height as f64) / 2.0,
                    image.origin[1] + (image.u[1] * image.width as f64 + image.v[1] * image.height as f64) / 2.0,
                ];
                if !in_area(mid) {
                    continue;
                }
                result.images.push(ImageImport {
                    rgba: image.rgba.clone(),
                    width: image.width,
                    height: image.height,
                    insertion: world3(image.origin),
                    u: turn(image.u),
                    v: turn(image.v),
                    layer: naming.layer(pdf_layer, Kind::Image),
                });
            }
        }
    }

    // The joined strokes, then (when asked) dashes read as one dashed line.
    let strokes = join_strokes(strokes);
    let strokes: Vec<(Stroke, Option<f64>)> = if settings.linetypes {
        infer_dashes(strokes)
    } else {
        strokes.into_iter().map(|s| (s, None)).collect()
    };
    for (stroke, dash) in strokes {
        let mut pl = LwPolyline::from_points(stroke.points.iter().map(|p| world2(*p)).collect());
        pl.is_closed = stroke.closed;
        pl.elevation = z;
        (pl.common.layer, pl.common.color, pl.common.line_weight) =
            (stroke.layer, stroke.color, stroke.weight);
        if let Some(dash) = dash {
            pl.common.linetype = DASH_LINETYPE.to_string();
            pl.common.linetype_scale = dash * scale;
            result.dashed = true;
        }
        result.entities.push(EntityType::LwPolyline(pl));
    }
    result.entities.extend(curves);
    result.layers = layer_colors;
    for image in &result.images {
        if !result.layers.iter().any(|(n, _)| *n == image.layer) {
            result.layers.push((image.layer.clone(), Color::Index(7)));
        }
    }
    result
}

/// One cubic B-spline through a subpath's Bézier control points (lines as
/// degree-elevated cubics).
fn spline_of(sp: &SubPath, world3: &dyn Fn([f64; 2]) -> Vector3) -> Spline {
    let mut spline = Spline::new();
    spline.degree = 3;
    let mut controls: Vec<Vector3> = Vec::new();
    for (i, seg) in sp.segments.iter().enumerate() {
        let (p0, p1, p2, p3) = match seg {
            Segment::Cubic(p0, p1, p2, p3) => (*p0, *p1, *p2, *p3),
            Segment::Line(a, b) => (
                *a,
                [a[0] + (b[0] - a[0]) / 3.0, a[1] + (b[1] - a[1]) / 3.0],
                [a[0] + (b[0] - a[0]) * 2.0 / 3.0, a[1] + (b[1] - a[1]) * 2.0 / 3.0],
                *b,
            ),
        };
        let pts = if i == 0 { vec![p0, p1, p2, p3] } else { vec![p1, p2, p3] };
        controls.extend(pts.into_iter().map(world3));
    }
    let spans = sp.segments.len();
    let mut knots = vec![0.0; 4];
    for k in 1..spans {
        knots.extend([k as f64; 3]);
    }
    knots.extend([spans as f64; 4]);
    spline.control_points = controls;
    spline.weights = Vec::new();
    spline.knots = knots;
    spline.flags.closed = sp.closed;
    spline
}

/// A filled area: a triangle or four-cornered straight loop is a SOLID; any
/// other loop, and every loop but a triangle when fills become hatches, one
/// solid HATCH.
fn fill_entities(
    path: &PdfPath,
    hatches: bool,
    world2: &dyn Fn([f64; 2]) -> Vector2,
    world3: &dyn Fn([f64; 2]) -> Vector3,
    z: f64,
) -> Vec<EntityType> {
    let straight = |sp: &SubPath| sp.segments.iter().all(|s| matches!(s, Segment::Line(..)));
    let corners = |sp: &SubPath| {
        let mut pts: Vec<[f64; 2]> = sp.segments.iter().map(|s| s.start()).collect();
        pts.dedup_by(|a, b| same_point(*a, *b));
        if pts.len() > 1 && same_point(pts[0], *pts.last().unwrap()) {
            pts.pop();
        }
        pts
    };
    if path.subpaths.len() == 1 && straight(&path.subpaths[0]) {
        let pts = corners(&path.subpaths[0]);
        match pts.len() {
            3 => {
                return vec![EntityType::Solid(Solid::triangle(
                    world3(pts[0]),
                    world3(pts[1]),
                    world3(pts[2]),
                ))]
            }
            4 if !hatches => {
                // SOLID corners run 1-2-4-3 (the second edge crosses).
                return vec![EntityType::Solid(Solid::new(
                    world3(pts[0]),
                    world3(pts[1]),
                    world3(pts[3]),
                    world3(pts[2]),
                ))];
            }
            _ => {}
        }
    }
    let mut hatch = Hatch::solid();
    for sp in &path.subpaths {
        let mut ring: Vec<Vector2> = Vec::new();
        for seg in &sp.segments {
            let steps = if matches!(seg, Segment::Cubic(..)) { 8 } else { 1 };
            for k in 0..steps {
                ring.push(world2(bezier(seg, k as f64 / steps as f64)));
            }
        }
        if ring.len() < 3 {
            continue;
        }
        let mut boundary = BoundaryPath::external();
        boundary.flags.set_polyline(true);
        boundary.edges.push(BoundaryEdge::Polyline(PolylineEdge::new(ring, true)));
        hatch.add_path(boundary);
    }
    if hatch.paths.is_empty() {
        return Vec::new();
    }
    hatch.elevation = z;
    vec![EntityType::Hatch(hatch)]
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["PDFIMPORT"]
});
