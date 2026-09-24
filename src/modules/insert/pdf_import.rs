// PDFIMPORT — turn the vector content of a PDF underlay into drawing objects.
//
//   Select PDF underlay or [File] <File>:
//   Specify first corner of area to import or [Polygonal/All/Settings] <All>:
//   Specify opposite corner:
//   Keep, Detach or Unload PDF underlay? [Keep/Detach/Unload] <Unload>:
//   Binding PDF file <path>, page <n> ...
//
// Geometry goes to PDF_Geometry and text to PDF_Text (the PDF has no layers
// of its own to use): chains of straight segments become polylines, a closed
// four-arc Bézier loop that is a circle becomes a CIRCLE, other curves become
// splines, filled areas solid hatches and text runs MTEXT in a "PDF <font>"
// text style.

use acadrust::entities::{
    AttachmentPoint, BoundaryEdge, BoundaryPath, Circle, Hatch, LwPolyline, MText, PolylineEdge,
    Spline, Underlay, UnderlayType,
};
use acadrust::types::{Color, Handle, LineWeight, Vector2, Vector3};
use acadrust::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::model::pdf_vector::{PageVectors, PdfPath, Segment, SubPath};
use crate::scene::model::wire_model::WireModel;

pub const GEOMETRY_LAYER: &str = "PDF_Geometry";
pub const TEXT_LAYER: &str = "PDF_Text";

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
            "S" | "SETTINGS" => {
                CmdResult::ReportError("PDF import settings are not available.".to_string())
            }
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

fn bezier(seg: &Segment, t: f64) -> [f64; 2] {
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
fn as_circle(sp: &SubPath) -> Option<([f64; 2], f64)> {
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

/// The objects an import of `page` creates for an underlay, with the
/// area in world coordinates.
pub fn convert(page: &PageVectors, underlay: &Underlay, area: &ImportArea) -> ImportResult {
    let world = |p: [f64; 2]| crate::entities::underlay::local_to_world(underlay, p);
    let world2 = |p: [f64; 2]| {
        let w = world(p);
        Vector2::new(w[0], w[1])
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

    let mut result = ImportResult::default();
    let mut geometry_color: Option<Color> = None;
    let mut text_color: Option<Color> = None;
    // The layer takes the colour of its first object; objects of that
    // colour then follow the layer.
    fn push(
        result: &mut ImportResult,
        mut entity: EntityType,
        color: Color,
        layer: &str,
        layer_color: &mut Option<Color>,
    ) {
        let layer_color = *layer_color.get_or_insert(color);
        let common = entity.common_mut();
        common.layer = layer.to_string();
        common.color = if color == layer_color { Color::ByLayer } else { color };
        result.entities.push(entity);
    }

    for path in &page.paths {
        if !path_in_area(path, area_local.as_deref()) {
            continue;
        }
        let (color, weight) = match (path.stroke, path.fill) {
            (Some((rgb, width)), _) => (color_of(rgb), lineweight_of(width)),
            (None, Some(rgb)) => (color_of(rgb), LineWeight::ByLayer),
            (None, None) => continue,
        };
        if path.stroke.is_none() {
            // A filled area: one solid hatch over its loops.
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
                continue;
            }
            hatch.elevation = z;
            push(&mut result, EntityType::Hatch(hatch), color, GEOMETRY_LAYER, &mut geometry_color);
            continue;
        }
        for sp in &path.subpaths {
            let mut entity = if let Some((c, r)) = as_circle(sp) {
                let w = world(c);
                EntityType::Circle(Circle::from_center_radius(
                    Vector3::new(w[0], w[1], z),
                    r * scale,
                ))
            } else if sp.segments.iter().all(|s| matches!(s, Segment::Line(..))) {
                let mut points: Vec<Vector2> = vec![world2(sp.segments[0].start())];
                points.extend(sp.segments.iter().map(|s| world2(s.end())));
                if sp.closed && points.len() > 2 {
                    points.pop();
                }
                let mut pl = LwPolyline::from_points(points);
                pl.is_closed = sp.closed;
                pl.elevation = z;
                EntityType::LwPolyline(pl)
            } else {
                // Mixed or curved: one cubic B-spline through the Bézier
                // control points (lines as degree-elevated cubics).
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
                    for p in pts {
                        let w = world(p);
                        controls.push(Vector3::new(w[0], w[1], z));
                    }
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
                EntityType::Spline(spline)
            };
            entity.common_mut().line_weight = weight;
            push(&mut result, entity, color, GEOMETRY_LAYER, &mut geometry_color);
        }
    }

    for text in &page.texts {
        let cap = if text.cap_height > 0.0 { text.cap_height } else { 0.0 };
        let probe = [text.origin[0] + text.width / 2.0, text.origin[1] + cap / 2.0];
        if let Some(area) = &area_local {
            if !inside(area, probe) && !inside(area, text.origin) {
                continue;
            }
        }
        let style = format!("PDF {}", text.font);
        if !result.text_styles.iter().any(|(name, _)| *name == style) {
            result.text_styles.push((style.clone(), font_file_for(&text.font)));
        }
        let (dx, dy) = (-text.rotation.sin(), text.rotation.cos());
        let mid = [text.origin[0] + dx * cap / 2.0, text.origin[1] + dy * cap / 2.0];
        let w = world(mid);
        let mut mtext = MText::new();
        mtext.value = text.text.trim_end().to_string();
        mtext.insertion_point = Vector3::new(w[0], w[1], z);
        mtext.height = cap * scale;
        mtext.rectangle_width = 0.0;
        mtext.rotation = text.rotation + underlay.rotation;
        mtext.style = style;
        mtext.attachment_point = AttachmentPoint::MiddleLeft;
        push(&mut result, EntityType::MText(mtext), color_of(text.color), TEXT_LAYER, &mut text_color);
    }

    if let Some(color) = geometry_color {
        result.layers.push((GEOMETRY_LAYER.to_string(), color));
    }
    if let Some(color) = text_color {
        result.layers.push((TEXT_LAYER.to_string(), color));
    }
    result
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["PDFIMPORT"]
});
