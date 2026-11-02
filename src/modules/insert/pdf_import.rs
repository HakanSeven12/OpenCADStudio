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
// Bézier that is a circular arc an arc segment of them (with "Join" on) or
// an ARC (off), a closed four-arc Bézier loop that is a circle a CIRCLE,
// other curves splines, filled triangles and quadrilaterals SOLIDs at 50% transparency
// (other fills solid hatches), text runs MTEXT in a "PDF <font>" style and
// raster images PNG files referenced by IMAGE objects.

use codec::entities::{
    Arc, AttachmentPoint, BoundaryEdge, BoundaryPath, Circle, Hatch, LwPolyline, MText,
    PolylineEdge, Solid, Spline, Underlay, UnderlayType,
};
use codec::types::{Color, Handle, LineWeight, Vector2, Vector3};
use codec::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::model::pdf_vector::{
    bezier, circle_of as as_circle, PageVectors, PdfPath, Segment, SubClose, SubPath,
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

static IMAGE_PATH: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// PDFIMPORTIMAGEPATH: the folder raster images are extracted to — relative
/// to the folder of the PDF, absolute as given, empty for that folder itself.
pub fn image_path() -> String {
    IMAGE_PATH
        .lock()
        .ok()
        .and_then(|p| p.clone())
        .unwrap_or_else(|| "PDF Images".to_string())
}

pub fn set_image_path(path: String) {
    if let Ok(mut p) = IMAGE_PATH.lock() {
        *p = Some(path);
    }
}

/// The extraction folder for the PDF at `pdf`.
pub fn image_dir(pdf: &str) -> Option<std::path::PathBuf> {
    let setting = image_path();
    let folder = std::path::Path::new(&setting);
    if folder.is_absolute() {
        return Some(folder.to_path_buf());
    }
    let parent = std::path::Path::new(pdf).parent()?;
    Some(if setting.is_empty() {
        parent.to_path_buf()
    } else {
        parent.join(folder)
    })
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
    /// Where the page's content goes: one drawing unit per page inch times
    /// the scale, or, on a page with a viewport measure (a plot), its
    /// measured units (a 1:1 millimetre plot comes in at its millimetres);
    /// a page's user unit multiplies either.
    pub fn placement(&self) -> Underlay {
        let mut u = Underlay::pdf();
        u.insertion_point = Vector3::new(self.insertion.x, self.insertion.y, self.insertion.z);
        let units = crate::scene::model::pdf_raster::page_units_per_inch(&self.path, &self.page)
            .unwrap_or(1.0);
        u.set_scale(self.scale * units);
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
    // The File option on the command line (-PDFIMPORT, or FILEDIA 0).
    FileName,
    Page,
    PageList,
    Insertion,
    Scale,
    Rotation,
}

/// What a file name that is no readable PDF (damaged, empty, not a PDF)
/// prints before the name is asked for again.
const UNREADABLE_FILE: &str = "This file cannot be opened.\n\
The problem can be caused by several things:\n\
- The data in the file is corrupted\n\
- The file is an earlier format that is no longer used\n\
- The file format is not supported\n\
Try to obtain a better copy of the file, recreate the file using different settings, \
or convert the file to a supported file format.";

/// A typed page number: digits only (no sign, decimals or ranges); one too
/// large for a 32-bit count is the largest one.
fn page_number(token: &str) -> Option<usize> {
    if token.is_empty() || !token.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(token.parse::<u64>().map_or(i32::MAX as u64, |n| n.min(i32::MAX as u64)) as usize)
}

pub struct PdfImportCommand {
    step: Step,
    handle: Handle,
    picked: Option<EntityType>,
    points: Vec<DVec3>,
    area: ImportArea,
    /// -PDFIMPORT: the file is always asked on the command line.
    dash: bool,
    file: PdfFileImport,
    pages: usize,
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
            dash: false,
            file: PdfFileImport {
                path: String::new(),
                page: "1".to_string(),
                scale: 1.0,
                rotation: 0.0,
                insertion: DVec3::ZERO,
            },
            pages: 0,
        }
    }

    /// -PDFIMPORT.
    pub fn command_line() -> Self {
        Self {
            dash: true,
            ..Self::new()
        }
    }

    /// The File option: the file dialog while FILEDIA is on (PDFIMPORT
    /// only), otherwise the file name prompt.
    fn file_option(&mut self) -> CmdResult {
        if !self.dash && crate::sys::file_dia() {
            return CmdResult::Dispatch("_PDFIMPORTFILE".to_string());
        }
        self.step = Step::FileName;
        CmdResult::NeedPoint
    }

    fn file_name(&mut self, text: &str) -> CmdResult {
        let typed = text.trim().trim_matches('"');
        if typed.is_empty() {
            return CmdResult::ReportError("Filename cannot be blank".to_string());
        }
        // A name without an extension is a .pdf file.
        let path = if std::path::Path::new(typed).extension().is_none() {
            format!("{typed}.pdf")
        } else {
            typed.to_string()
        };
        let shown = crate::entities::underlay::display_path(&path);
        let Ok(bytes) = std::fs::read(&path) else {
            return CmdResult::ReportError(format!("File {shown} does not exist."));
        };
        // A damaged file is repaired only when it still has its trailer (the
        // `trailer` dictionary or a cross-reference stream); a cut-off one
        // cannot be opened.
        // ponytail: byte search; a compressed stream holding these bytes
        // would pass the test.
        let has = |key: &[u8]| bytes.windows(key.len()).any(|w| w == key);
        if !has(b"trailer") && !has(b"/XRef") {
            return CmdResult::ReportError(UNREADABLE_FILE.to_string());
        }
        crate::scene::model::pdf_raster::register_source(&path, std::sync::Arc::new(bytes));
        let notice = match crate::scene::model::pdf_password::prompt_gate(&path, text, false) {
            Ok(notice) => notice,
            Err(ask) => return ask,
        };
        let Some(pages) =
            crate::scene::model::pdf_raster::page_count(&path).filter(|&pages| pages > 0)
        else {
            return CmdResult::ReportError(UNREADABLE_FILE.to_string());
        };
        self.pages = pages;
        self.file.path = path;
        self.step = Step::Page;
        notice.map_or(CmdResult::NeedPoint, CmdResult::ReportMeasurement)
    }

    fn page(&mut self, text: &str) -> CmdResult {
        let token = text.trim();
        if token == "?" {
            self.step = Step::PageList;
            return CmdResult::NeedPoint;
        }
        let page = if token.is_empty() {
            1
        } else {
            match page_number(token) {
                // Zero is asked again without a message.
                Some(0) => return CmdResult::NeedPoint,
                Some(n) => n,
                None => {
                    return CmdResult::ReportError("Please enter page number or '?'.".to_string())
                }
            }
        };
        if page > self.pages {
            let shown = crate::entities::underlay::display_path(&self.file.path);
            return CmdResult::ReportError(format!("There is no {page} in {shown}."));
        }
        self.file.page = page.to_string();
        self.step = Step::Insertion;
        CmdResult::NeedPoint
    }

    fn page_list(&mut self, text: &str) -> CmdResult {
        let token = text.trim();
        if token.is_empty() || token == "*" {
            self.step = Step::Page;
            let all: Vec<String> = (1..=self.pages).map(|n| n.to_string()).collect();
            return CmdResult::ReportMeasurement(all.join("\n"));
        }
        match page_number(token) {
            // Zero, or anything but a page number, is asked again.
            Some(0) => CmdResult::NeedPoint,
            None => CmdResult::ReportError("Please enter page number or *.".to_string()),
            Some(n) => {
                self.step = Step::Page;
                CmdResult::ReportMeasurement(if n <= self.pages {
                    format!("Found page {n}.")
                } else {
                    "No matching page found.".to_string()
                })
            }
        }
    }

    fn scale(&mut self, value: f64) -> CmdResult {
        if value <= 0.0 {
            return CmdResult::ReportError("Value must be positive and nonzero.".to_string());
        }
        self.file.scale = value;
        self.step = Step::Rotation;
        CmdResult::NeedPoint
    }

    fn rotation(&mut self, radians: f64) -> CmdResult {
        self.file.rotation = radians;
        CmdResult::PdfImportFile(self.file.clone())
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
        if self.dash {
            "-PDFIMPORT"
        } else {
            "PDFIMPORT"
        }
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Select => "Select PDF underlay or [File] <File>:".to_string(),
            Step::Area => {
                "Specify first corner of area to import or [Polygonal/All/Settings] <All>:"
                    .to_string()
            }
            Step::Opposite => "Specify opposite corner:".to_string(),
            Step::Polygon if self.points.is_empty() => "Specify first point:".to_string(),
            Step::Polygon => "Specify next point or [Undo]:".to_string(),
            Step::Mode => {
                "Keep, Detach or Unload PDF underlay? [Keep/Detach/Unload] <Unload>:".to_string()
            }
            Step::FileName => "Enter filename for PDF import:".to_string(),
            Step::Page => "Enter page number or [?] <1>:".to_string(),
            Step::PageList => "Enter page to list <*>:".to_string(),
            Step::Insertion => "Specify insertion point <0,0>:".to_string(),
            Step::Scale => "Specify scale factor <1.0>:".to_string(),
            Step::Rotation => "Specify rotation <0>:".to_string(),
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
            Step::Mode | Step::Page | Step::PageList => InputKind::SingleToken,
            Step::FileName => InputKind::FreeText,
            _ => InputKind::Point,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(
            self.step,
            Step::Select | Step::Area | Step::Polygon | Step::Scale | Step::Rotation
        )
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
            Step::Insertion => {
                self.file.insertion = pt;
                self.step = Step::Scale;
                CmdResult::NeedPoint
            }
            // A second point gives the scale, or the angle, from the
            // insertion point.
            Step::Scale => {
                let d = (pt - self.file.insertion).truncate().length();
                self.scale(d)
            }
            Step::Rotation => {
                let d = (pt - self.file.insertion).truncate();
                self.rotation(d.y.atan2(d.x))
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let token = text.trim().to_ascii_uppercase();
        Some(match self.step {
            Step::Select if matches!(token.as_str(), "F" | "FILE" | "_F" | "_FILE") => {
                self.file_option()
            }
            Step::Area => self.area_choice(&token),
            Step::Polygon if token == "U" || token == "UNDO" => {
                self.points.pop();
                CmdResult::NeedPoint
            }
            Step::Mode => self.mode_choice(&token),
            Step::FileName => self.file_name(text),
            Step::Page => self.page(text),
            Step::PageList => self.page_list(text),
            Step::Scale => match token.parse::<f64>() {
                Ok(v) => self.scale(v),
                Err(_) if token.is_empty() => self.scale(1.0),
                Err(_) => CmdResult::ReportError(
                    "Requires numeric distance or second point.".to_string(),
                ),
            },
            Step::Rotation => match token.parse::<f64>() {
                Ok(v) => self.rotation(v.to_radians()),
                Err(_) if token.is_empty() => self.rotation(0.0),
                Err(_) => CmdResult::ReportError(
                    "Requires valid numeric angle or second point.".to_string(),
                ),
            },
            _ => return None,
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Select => self.file_option(),
            Step::Area => self.area_choice(""),
            Step::Polygon if self.points.len() >= 3 => {
                self.area = ImportArea::Polygon(self.points.clone());
                self.step = Step::Mode;
                CmdResult::NeedPoint
            }
            Step::Polygon => CmdResult::NeedPoint,
            Step::Mode => self.mode_choice(""),
            Step::Opposite => CmdResult::NeedPoint,
            Step::FileName => self.file_name(""),
            Step::Page => self.page(""),
            Step::PageList => self.page_list(""),
            Step::Insertion => self.on_point(DVec3::ZERO),
            Step::Scale => self.scale(1.0),
            Step::Rotation => self.rotation(0.0),
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let ring: Vec<DVec3> = match self.step {
            Step::Opposite => {
                let a = self.points[0];
                vec![
                    a,
                    DVec3::new(pt.x, a.y, a.z),
                    pt,
                    DVec3::new(a.x, pt.y, a.z),
                ]
            }
            Step::Polygon if !self.points.is_empty() => {
                let mut ring = self.points.clone();
                ring.push(pt);
                ring
            }
            _ => return None,
        };
        let mut points: Vec<[f64; 3]> = ring.iter().map(|p| p.to_array()).collect();
        points.push(points[0]);
        Some(WireModel::solid_f64(
            "pdf_import_area".into(),
            points,
            WireModel::CYAN,
            false,
        ))
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
        // Black is 7; white stays a true colour.
        [0, 0, 0] => Color::Index(7),
        [r, g, b] => Color::Rgb { r, g, b },
    }
}

/// Stroke width in points → the nearest standard lineweight.
fn lineweight_of(width_pt: f64) -> LineWeight {
    const STANDARD: [i16; 24] = [
        0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158,
        200, 211,
    ];
    // A zero-width (thinnest) line is the 0.00 mm weight.
    if width_pt <= 0.0 {
        return LineWeight::Value(0);
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

/// The font file of a PDF font's text style: the installed face of the
/// family of exactly its name, or of the family the system substitutes for
/// it (Helvetica: Arial, Times: Times New Roman, Courier: Courier New),
/// else Arial.
fn font_file_for(font: &str) -> String {
    use crate::scene::text::sysfont::face_file;
    const SUBSTITUTES: [(&str, &str); 3] = [
        ("Helvetica", "Arial"),
        ("Times", "Times New Roman"),
        ("Courier", "Courier New"),
    ];
    let family = SUBSTITUTES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(font))
        .map_or(font, |(_, to)| *to);
    face_file(family, false, false).unwrap_or_else(|| "arial.ttf".to_string())
}

/// The height of the capital A over the em in the installed font of the
/// family of exactly `family`, the reference's capital height for text in
/// it; `None` when no installed family has that name.
fn capital_a_height(family: &str) -> Option<f64> {
    use crate::scene::text::sysfont::{face_file, with_face_data};
    face_file(family, false, false)?;
    with_face_data(family, |data, index| {
        let face = ttf_parser::Face::parse(data, index).ok()?;
        let glyph = face.glyph_index('A')?;
        let bounds = face.glyph_bounding_box(glyph)?;
        Some(bounds.y_max as f64 / face.units_per_em() as f64)
    })
    .flatten()
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
    /// The text styles the drawing has: name and font file.
    pub styles: Vec<(String, String)>,
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

/// A stroke before it becomes an object: chains can still be joined or read
/// as dashes. `bulges[i]` bends the segment from point i to the next (the
/// closing one on a closed stroke; 0 after an open stroke's last point).
struct Stroke {
    /// Drawing position: the objects are created in drawing order.
    order: usize,
    layer: String,
    color: Color,
    weight: LineWeight,
    points: Vec<[f64; 2]>,
    bulges: Vec<f64>,
    closed: bool,
}

impl Stroke {
    fn straight(&self) -> bool {
        self.bulges.iter().all(|b| *b == 0.0)
    }

    /// Whether all its points are one point.
    fn zero(&self) -> bool {
        self.points.iter().all(|p| same_point(*p, self.points[0]))
    }

    fn reverse(&mut self) {
        self.points.reverse();
        let n = self.bulges.len();
        let mut bulges: Vec<f64> = self.bulges[..n - 1].iter().rev().map(|b| -b).collect();
        bulges.push(0.0);
        self.bulges = bulges;
    }
}

/// The bulge of a Bézier that is a circular arc, from its midpoint's height
/// over the chord (positive counter-clockwise): its quarter and
/// three-quarter points lie on the circle through its ends and midpoint
/// within a thousandth of the chord, as the reference reads arcs.
fn arc_bulge(seg: &Segment) -> Option<f64> {
    let Segment::Cubic(p0, _, _, p3) = seg else {
        return None;
    };
    let m = bezier(seg, 0.5);
    let (ax, ay) = (m[0] - p0[0], m[1] - p0[1]);
    let (bx, by) = (p3[0] - p0[0], p3[1] - p0[1]);
    let d = 2.0 * (ax * by - ay * bx);
    let chord = bx.hypot(by);
    if d.abs() < 1e-12 || chord < 1e-9 {
        return None;
    }
    // Circle through p0, m, p3 (relative to p0).
    let (a2, b2) = (ax * ax + ay * ay, bx * bx + by * by);
    let c = [(by * a2 - ay * b2) / d, (ax * b2 - bx * a2) / d];
    let r = c[0].hypot(c[1]);
    let tol = chord * 1e-3;
    let on = [0.25, 0.75].iter().all(|t| {
        let q = bezier(seg, *t);
        ((q[0] - p0[0] - c[0]).hypot(q[1] - p0[1] - c[1]) - r).abs() <= tol
    });
    if !on {
        return None;
    }
    let h = (ax * by - ay * bx) / chord;
    Some(2.0 * h / chord)
}

/// A subpath cut where its curves are not circular arcs.
enum Piece {
    /// Points, bulges (as in [`Stroke`]) and whether it closes.
    Run(Vec<[f64; 2]>, Vec<f64>, bool),
    /// A cubic that is no circular arc.
    Curve(Segment),
}

/// The straight-and-arc runs of a subpath between its other curves. A
/// subpath without other curves is one run (closed when it closes). With
/// other curves, a close operator that has to draw a line back to the
/// subpath's start closes the run after the last of them on that run's own
/// start instead (as the reference reads it). A segment on one point adds
/// nothing; a run wholly on one point is a zero-length two-point run, never
/// closed.
fn run_strokes(sp: &SubPath, close: Option<SubClose>) -> Vec<Piece> {
    let whole = !sp
        .segments
        .iter()
        .any(|s| matches!(s, Segment::Cubic(..)) && arc_bulge(s).is_none());
    let close = close.unwrap_or(SubClose {
        op: sp.closed,
        line: false,
    });
    let segments = match (whole, close.line) {
        (false, true) => &sp.segments[..sp.segments.len() - 1],
        _ => &sp.segments[..],
    };
    let mut out = Vec::new();
    let mut points: Vec<[f64; 2]> = Vec::new();
    let mut bulges: Vec<f64> = Vec::new();
    let flush = |points: &mut Vec<[f64; 2]>,
                 bulges: &mut Vec<f64>,
                 out: &mut Vec<Piece>,
                 closed: bool| {
        if points.len() == 1 {
            // Only zero-length segments: a line from the point to itself.
            out.push(Piece::Run(vec![points[0], points[0]], vec![0.0, 0.0], false));
        } else if points.len() > 1 {
            bulges.push(0.0);
            out.push(Piece::Run(
                std::mem::take(points),
                std::mem::take(bulges),
                closed,
            ));
        }
        points.clear();
        bulges.clear();
    };
    for seg in segments {
        let bulge = match seg {
            Segment::Line(..) => Some(0.0),
            cubic => arc_bulge(cubic),
        };
        match bulge {
            Some(b) => {
                if points.is_empty() {
                    points.push(seg.start());
                }
                if b == 0.0 && same_point(seg.start(), seg.end()) {
                    continue;
                }
                points.push(seg.end());
                bulges.push(b);
            }
            None => {
                flush(&mut points, &mut bulges, &mut out, false);
                out.push(Piece::Curve(seg.clone()));
            }
        }
    }
    if whole && sp.closed && points.len() > 2 {
        // The closing point is the first again.
        points.pop();
        out.push(Piece::Run(points, bulges, true));
    } else {
        let closed = !whole && close.line && points.len() > 1;
        flush(&mut points, &mut bulges, &mut out, closed);
    }
    out
}

/// Straight runs cut to the convex clip they reach out of (a run leaving
/// and entering again splits); runs with arcs and other curves are kept.
// ponytail: only straight runs are cut; arcs and Bézier curves crossing the
// clip stay whole.
fn clip_runs(pieces: Vec<Piece>, clip: &[[f64; 2]]) -> Vec<Piece> {
    use crate::scene::model::pdf_vector::clip_segment;
    let mut out = Vec::new();
    for piece in pieces {
        let (points, closed) = match &piece {
            Piece::Run(points, bulges, closed)
                if bulges.iter().all(|b| *b == 0.0)
                    && !points.iter().all(|p| same_point(*p, points[0])) =>
            {
                (points.clone(), *closed)
            }
            _ => {
                out.push(piece);
                continue;
            }
        };
        let mut ring = points.clone();
        if closed {
            ring.push(points[0]);
        }
        let mut run: Vec<[f64; 2]> = Vec::new();
        let flush = |run: &mut Vec<[f64; 2]>, out: &mut Vec<Piece>| {
            if run.len() > 1 {
                let n = run.len();
                out.push(Piece::Run(std::mem::take(run), vec![0.0; n], false));
            }
            run.clear();
        };
        for w in ring.windows(2) {
            match clip_segment(w[0], w[1], clip) {
                Some((a, b)) => {
                    if run.last().is_none_or(|last| !same_point(*last, a)) {
                        flush(&mut run, &mut out);
                        run.push(a);
                    }
                    run.push(b);
                }
                None => flush(&mut run, &mut out),
            }
        }
        flush(&mut run, &mut out);
    }
    out
}

/// Circle through three points: centre and radius.
fn circle_through(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<([f64; 2], f64)> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (
        a[0] * a[0] + a[1] * a[1],
        b[0] * b[0] + b[1] * b[1],
        c[0] * c[0] + c[1] * c[1],
    );
    let x = (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d;
    let y = (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d;
    Some(([x, y], (a[0] - x).hypot(a[1] - y)))
}

/// An ellipse: centre, unit major direction, semi-axes.
struct Conic {
    center: [f64; 2],
    major: [f64; 2],
    a: f64,
    b: f64,
}

impl Conic {
    /// The ellipse through five points (none at their centroid).
    fn through(points: &[[f64; 2]; 5]) -> Option<Self> {
        let o = [
            points.iter().map(|p| p[0]).sum::<f64>() / 5.0,
            points.iter().map(|p| p[1]).sum::<f64>() / 5.0,
        ];
        // A x² + B xy + C y² + D x + E y = 1 about the centroid.
        let mut m = [[0.0f64; 6]; 5];
        for (row, p) in m.iter_mut().zip(points) {
            let (x, y) = (p[0] - o[0], p[1] - o[1]);
            *row = [x * x, x * y, y * y, x, y, 1.0];
        }
        for col in 0..5 {
            let pivot = (col..5).max_by(|i, j| m[*i][col].abs().total_cmp(&m[*j][col].abs()))?;
            m.swap(col, pivot);
            if m[col][col].abs() < 1e-18 {
                return None;
            }
            for row in 0..5 {
                if row != col {
                    let f = m[row][col] / m[col][col];
                    for k in col..6 {
                        m[row][k] -= f * m[col][k];
                    }
                }
            }
        }
        let s: Vec<f64> = (0..5).map(|i| m[i][5] / m[i][i]).collect();
        let (a, b, c, d, e) = (s[0], s[1], s[2], s[3], s[4]);
        let det = 4.0 * a * c - b * b;
        if det <= 0.0 {
            return None;
        }
        let x0 = (b * e - 2.0 * c * d) / det;
        let y0 = (b * d - 2.0 * a * e) / det;
        let f = a * x0 * x0 + b * x0 * y0 + c * y0 * y0 + d * x0 + e * y0 - 1.0;
        if f >= 0.0 {
            return None;
        }
        // Eigen-directions of the quadratic part; the smaller value is the
        // major axis.
        let theta = 0.5 * b.atan2(a - c);
        let (ct, st) = (theta.cos(), theta.sin());
        let q1 = a * ct * ct + b * ct * st + c * st * st;
        let q2 = a * st * st - b * ct * st + c * ct * ct;
        let (r1, r2) = ((-f / q1).sqrt(), (-f / q2).sqrt());
        let (major, ra, rb) = if r1 >= r2 {
            ([ct, st], r1, r2)
        } else {
            ([-st, ct], r2, r1)
        };
        Some(Self {
            center: [x0 + o[0], y0 + o[1]],
            major,
            a: ra,
            b: rb,
        })
    }

    /// (u, v) of a point in the ellipse's own frame, scaled to the unit
    /// circle.
    fn unit(&self, p: [f64; 2]) -> (f64, f64) {
        let (dx, dy) = (p[0] - self.center[0], p[1] - self.center[1]);
        let u = dx * self.major[0] + dy * self.major[1];
        let v = -dx * self.major[1] + dy * self.major[0];
        (u / self.a, v / self.b)
    }
}

/// Turning of a run of points about a centre: positive counter-clockwise.
fn sweep_sign(points: &[[f64; 2]], c: [f64; 2]) -> f64 {
    points
        .windows(2)
        .map(|w| (w[0][0] - c[0]) * (w[1][1] - c[1]) - (w[0][1] - c[1]) * (w[1][0] - c[0]))
        .sum::<f64>()
        .signum()
}

/// Minimum segments of a straight polyline read as a curve.
const CURVE_MIN_SEGMENTS: usize = 9;
/// Share of the vertices that must lie within tolerance of the curve.
const CURVE_INLIERS: f64 = 0.85;
/// Radial tolerance of an arc through the first, middle and last vertex
/// (every vertex lies within it).
const ARC_TOLERANCE: f64 = 0.02;
/// Tolerance of an ellipse through five vertices (in its unit circle).
const ELLIPSE_TOLERANCE: f64 = 0.015;
/// An ellipse at least this round is a circle.
const CIRCLE_RATIO: f64 = 0.96;

/// A joined stroke read as a curve, in drawing coordinates (`points`):
/// a run of arcs on one circle is an ARC (a CIRCLE when it closes); a
/// straight polyline of nine or more segments whose vertices lie on a
/// circular arc an ARC, on an ellipse a CIRCLE (round) or an ELLIPSE (an
/// elliptical arc when open).
// ponytail: tolerances fitted to measured samples (glyph outlines, noisy
// and exact polygons); an ellipse allows 15% of its vertices off it, an arc
// none.
fn curve_of(points: &[[f64; 2]], bulges: &[f64], closed: bool, z: f64) -> Option<EntityType> {
    use std::f64::consts::TAU;
    let n = points.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    if segs == 0 {
        return None;
    }
    let arc = |c: [f64; 2], r: f64, first: [f64; 2], last: [f64; 2], ccw: bool| {
        let angle = |p: [f64; 2]| (p[1] - c[1]).atan2(p[0] - c[0]).rem_euclid(TAU);
        let (start, end) = if ccw {
            (angle(first), angle(last))
        } else {
            (angle(last), angle(first))
        };
        EntityType::Arc(Arc::from_center_radius_angles(
            Vector3::new(c[0], c[1], z),
            r,
            start,
            end,
        ))
    };
    // Arcs of one circle, all turning the same way.
    if bulges[..segs].iter().all(|b| *b != 0.0) {
        let turn = bulges[0].signum();
        let arcs: Vec<_> = (0..segs)
            .map(|i| {
                crate::entities::common::BulgeArc::from_bulge(
                    points[i],
                    points[(i + 1) % n],
                    bulges[i],
                )
            })
            .collect::<Option<_>>()?;
        let first = &arcs[0];
        let same = arcs.iter().zip(&bulges[..segs]).all(|(a, b)| {
            b.signum() == turn
                && (a.radius - first.radius).abs() <= first.radius * 1e-4
                && (a.center[0] - first.center[0]).hypot(a.center[1] - first.center[1])
                    <= first.radius * 1e-4
        });
        if !same || segs < 2 {
            return None;
        }
        return Some(if closed {
            EntityType::Circle(Circle::from_center_radius(
                Vector3::new(first.center[0], first.center[1], z),
                first.radius,
            ))
        } else {
            arc(first.center, first.radius, points[0], points[n - 1], turn > 0.0)
        });
    }
    if bulges.iter().any(|b| *b != 0.0) || segs < CURVE_MIN_SEGMENTS {
        return None;
    }
    let enough = |inliers: usize| inliers as f64 >= CURVE_INLIERS * n as f64;
    if !closed {
        if let Some((c, r)) = circle_through(points[0], points[n / 2], points[n - 1]) {
            let on = points
                .iter()
                .all(|p| ((p[0] - c[0]).hypot(p[1] - c[1]) - r).abs() <= ARC_TOLERANCE * r);
            if on {
                return Some(arc(c, r, points[0], points[n - 1], sweep_sign(points, c) > 0.0));
            }
        }
    }
    // Five vertices spread along the run: a fifth of a closed loop apart,
    // or the ends and quarters of an open one.
    let step = if closed {
        (n as f64 / 5.0).round()
    } else {
        (n - 1) as f64 / 4.0
    };
    let idx: Vec<usize> = (0..5)
        .map(|k| ((k as f64 * step).round() as usize) % n)
        .collect();
    let five = [
        points[idx[0]],
        points[idx[1]],
        points[idx[2]],
        points[idx[3]],
        points[idx[4]],
    ];
    let e = Conic::through(&five)?;
    let inliers = points
        .iter()
        .filter(|p| {
            let (u, v) = e.unit(**p);
            (u.hypot(v) - 1.0).abs() <= ELLIPSE_TOLERANCE
        })
        .count();
    if !enough(inliers) {
        return None;
    }
    if closed && e.b / e.a >= CIRCLE_RATIO {
        let (c, r) = circle_through(five[0], five[2], five[4])?;
        return Some(EntityType::Circle(Circle::from_center_radius(
            Vector3::new(c[0], c[1], z),
            r,
        )));
    }
    let param = |p: [f64; 2]| {
        let (u, v) = e.unit(p);
        v.atan2(u).rem_euclid(TAU)
    };
    let (start, mut end) = if closed {
        let s = param(points[0]);
        (s, s + TAU)
    } else if sweep_sign(points, e.center) > 0.0 {
        (param(points[0]), param(points[n - 1]))
    } else {
        (param(points[n - 1]), param(points[0]))
    };
    if end <= start {
        end += TAU;
    }
    let mut ellipse = codec::entities::Ellipse::from_center_axes(
        Vector3::new(e.center[0], e.center[1], z),
        Vector3::new(e.major[0] * e.a, e.major[1] * e.a, 0.0),
        e.b / e.a,
    );
    ellipse.start_parameter = start;
    ellipse.end_parameter = end;
    Some(EntityType::Ellipse(ellipse))
}

fn same_point(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-7 && (a[1] - b[1]).abs() < 1e-7
}

/// Open strokes of one layer, colour and weight that meet end to end become
/// one polyline (closed when the chain returns to its start). Straight ones
/// always do; ones with arcs only with `join` (off, their arcs stay apart).
fn join_strokes(strokes: Vec<Stroke>, join: bool) -> Vec<Stroke> {
    let mut out: Vec<Stroke> = Vec::new();
    let mut open: Vec<Stroke> = Vec::new();
    for s in strokes {
        if s.closed || s.zero() || (!join && !s.straight()) {
            out.push(s);
        } else {
            open.push(s);
        }
    }
    // Endpoints by grid cell, so finding the next stroke of a chain is a
    // lookup rather than a scan of every open stroke: a dense page has
    // hundreds of thousands of them (#2285). A cell is the `same_point`
    // tolerance, so a match is in the point's cell or a neighbouring one.
    let cell = |p: [f64; 2]| ((p[0] / 1e-7).floor() as i64, (p[1] / 1e-7).floor() as i64);
    let mut ends: rustc_hash::FxHashMap<(i64, i64), Vec<usize>> = Default::default();
    for (index, stroke) in open.iter().enumerate() {
        for p in [stroke.points[0], *stroke.points.last().expect("strokes have points")] {
            ends.entry(cell(p)).or_default().push(index);
        }
    }
    let mut open: Vec<Option<Stroke>> = open.into_iter().map(Some).collect();
    // The earliest drawn unused stroke of the chain's kind with an end at `p`.
    let find = |open: &[Option<Stroke>], chain: &Stroke, p: [f64; 2]| {
        let (cx, cy) = cell(p);
        (cx - 1..=cx + 1)
            .flat_map(|x| (cy - 1..=cy + 1).map(move |y| (x, y)))
            .filter_map(|key| ends.get(&key))
            .flatten()
            .copied()
            .filter(|&index| {
                open[index].as_ref().is_some_and(|o| {
                    o.layer == chain.layer
                        && o.color == chain.color
                        && o.weight == chain.weight
                        && (same_point(o.points[0], p) || same_point(*o.points.last().unwrap(), p))
                })
            })
            .min()
    };
    // Drawing order is kept: dashes read as a run only in the order drawn.
    for first in 0..open.len() {
        let Some(mut chain) = open[first].take() else {
            continue;
        };
        loop {
            let end = *chain.points.last().expect("strokes have points");
            let start = chain.points[0];
            if let Some(i) = find(&open, &chain, end) {
                let mut next = open[i].take().expect("found strokes are unused");
                if !same_point(next.points[0], end) {
                    next.reverse();
                }
                chain.points.extend(next.points.into_iter().skip(1));
                chain.bulges.pop();
                chain.bulges.extend(next.bulges);
            } else if let Some(i) = find(&open, &chain, start) {
                let mut prev = open[i].take().expect("found strokes are unused");
                if !same_point(*prev.points.last().unwrap(), start) {
                    prev.reverse();
                }
                prev.points.pop();
                prev.points.extend(chain.points);
                chain.points = prev.points;
                prev.bulges.pop();
                prev.bulges.extend(chain.bulges);
                chain.bulges = prev.bulges;
            } else {
                break;
            }
        }
        if chain.points.len() > 3 && same_point(chain.points[0], *chain.points.last().unwrap()) {
            chain.points.pop();
            chain.bulges.pop();
            chain.closed = true;
        }
        out.push(chain);
    }
    // A chain takes the place of its first stroke.
    out.sort_by_key(|s| s.order);
    out
}

/// An open stroke cut into its straight runs and one stroke per arc.
fn split_at_arcs(stroke: Stroke) -> Vec<Stroke> {
    let mut out = Vec::new();
    let piece = |points: Vec<[f64; 2]>, bulges: Vec<f64>| Stroke {
        order: stroke.order,
        layer: stroke.layer.clone(),
        color: stroke.color,
        weight: stroke.weight,
        points,
        bulges,
        closed: false,
    };
    let mut run: Vec<[f64; 2]> = vec![stroke.points[0]];
    for i in 0..stroke.points.len() - 1 {
        let (a, b, bulge) = (stroke.points[i], stroke.points[i + 1], stroke.bulges[i]);
        if bulge == 0.0 {
            run.push(b);
            continue;
        }
        if run.len() > 1 {
            let n = run.len();
            out.push(piece(std::mem::take(&mut run), vec![0.0; n]));
        }
        out.push(piece(vec![a, b], vec![bulge, 0.0]));
        run = vec![b];
    }
    if run.len() > 1 {
        let n = run.len();
        out.push(piece(run, vec![0.0; n]));
    }
    out
}

/// Runs of three or more equal collinear dashes with equal gaps become one
/// line from the first dash's start to the last's end, with the dash length
/// (the linetype scale).
// ponytail: dashes are matched along one direction at a time in drawing
// order; patterns of mixed dash lengths stay separate lines.
fn infer_dashes(strokes: Vec<Stroke>) -> Vec<(Stroke, Option<f64>)> {
    let is_dash = |s: &Stroke| !s.closed && s.points.len() == 2 && s.straight();
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
    // Layers in the order their first object comes; the colours are settled
    // once every object is made (see `layer_colours_by_majority`).
    let mut layer_colors: Vec<(String, Color)> = Vec::new();
    let mut color_on = |layer: &str, color: Color| -> Color {
        if !layer_colors.iter().any(|(n, _)| n == layer) {
            layer_colors.push((layer.to_string(), color));
        }
        color
    };
    let fill_transparency = codec::types::Transparency::from_alpha_value(0x0200_007F);
    // A mirrored placement turns arcs the other way.
    let turn = (underlay.x_scale * underlay.y_scale).signum();
    let mut strokes: Vec<Stroke> = Vec::new();
    // Circles and splines, by drawing position like the strokes.
    let mut curves: Vec<(usize, EntityType)> = Vec::new();

    for (pdf_layer, page) in content {
        let pdf_layer = pdf_layer.as_deref();
        if settings.vector {
            for path in &page.paths {
                if !path_in_area(path, area_local.as_deref()) {
                    continue;
                }
                match (path.stroke, path.fill) {
                    // Filled (and maybe stroked by the same operator): the
                    // areas in the fill colour (a four-cornered SOLID at the
                    // stroke width current when painted), the lines and
                    // curves bounding no area in that stroke colour and width.
                    (stroke, Some(rgb)) if settings.fills => {
                        let (pen_rgb, pen_width) = path.pen.or(stroke).unwrap_or((rgb, -1.0));
                        let pen_weight = if settings.lineweights && pen_width >= 0.0 {
                            lineweight_of(pen_width)
                        } else {
                            LineWeight::ByLayer
                        };
                        let out = fill_entities(path, settings.hatches, turn, &world2, &world3, z);
                        let layer = naming.layer(pdf_layer, Kind::Fill);
                        let color = color_on(&layer, color_of(rgb));
                        for (mut entity, four) in out.fills {
                            let common = entity.common_mut();
                            (common.layer, common.color, common.transparency) =
                                (layer.clone(), color, fill_transparency);
                            if four {
                                common.line_weight = pen_weight;
                            }
                            result.entities.push(entity);
                        }
                        let layer = naming.layer(pdf_layer, Kind::Geometry);
                        let color = color_on(&layer, color_of(pen_rgb));
                        for mut entity in out.lines {
                            let common = entity.common_mut();
                            (common.layer, common.color, common.line_weight) =
                                (layer.clone(), color, pen_weight);
                            curves.push((strokes.len() + curves.len(), entity));
                        }
                    }
                    (Some((rgb, width)), _) => {
                        let layer = naming.layer(pdf_layer, Kind::Geometry);
                        let color = color_on(&layer, color_of(rgb));
                        let weight = if settings.lineweights {
                            lineweight_of(width)
                        } else {
                            LineWeight::ByLayer
                        };
                        for (k, sp) in path.subpaths.iter().enumerate() {
                            let order = strokes.len() + curves.len();
                            let close = path.closes.get(k).copied();
                            if let Some((c, r)) = as_circle(sp).filter(|_| path.clip.is_none()) {
                                let w = world(c);
                                let mut circle = EntityType::Circle(Circle::from_center_radius(
                                    Vector3::new(w[0], w[1], z),
                                    r * scale,
                                ));
                                let common = circle.common_mut();
                                (common.layer, common.color, common.line_weight) =
                                    (layer.clone(), color, weight);
                                curves.push((order, circle));
                            } else {
                                // Straight and circular runs become polylines;
                                // each other curve its own one-span SPLINE.
                                let mut all_runs = run_strokes(sp, close);
                                if let Some(clip) = &path.clip {
                                    all_runs = clip_runs(all_runs, clip);
                                }
                                for piece in all_runs {
                                    match piece {
                                        Piece::Run(points, bulges, closed) => {
                                            let stroke = Stroke {
                                                order,
                                                layer: layer.clone(),
                                                color,
                                                weight,
                                                points,
                                                bulges,
                                                closed,
                                            };
                                            if settings.join
                                                || stroke.straight()
                                                || stroke.closed
                                                || stroke.zero()
                                            {
                                                strokes.push(stroke);
                                            } else {
                                                // Join off: each arc apart, straight runs
                                                // between them.
                                                strokes.extend(split_at_arcs(stroke));
                                            }
                                        }
                                        Piece::Curve(seg) => {
                                            let one = SubPath {
                                                segments: vec![seg],
                                                closed: false,
                                            };
                                            let mut spline =
                                                EntityType::Spline(spline_of(&one, &world3));
                                            let common = spline.common_mut();
                                            (common.layer, common.color, common.line_weight) =
                                                (layer.clone(), color, weight);
                                            curves.push((order, spline));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        if settings.text {
            for text in &page.texts {
                let style = format!("PDF {}", text.font);
                // Arial stands in for a font no installed family answers
                // to; once a style of its name maps to Arial, its runs are
                // 0.72 of their size.
                let arial_style = result.text_styles.iter().chain(&naming.styles).any(
                    |(name, file)| {
                        name.eq_ignore_ascii_case(&style) && file.eq_ignore_ascii_case("arial.ttf")
                    },
                );
                let ratio = match capital_a_height(&text.font) {
                    Some(ratio) => ratio,
                    None if arial_style => 0.72,
                    None => capital_a_height("Arial").unwrap_or(0.72),
                };
                let cap = ratio * text.em;
                let probe = [
                    text.origin[0] + text.width / 2.0,
                    text.origin[1] + cap / 2.0,
                ];
                if !in_area(probe) && !in_area(text.origin) {
                    continue;
                }
                if !result.text_styles.iter().any(|(name, _)| *name == style) {
                    result
                        .text_styles
                        .push((style.clone(), font_file_for(&text.font)));
                }
                let (dx, dy) = (-text.rotation.sin(), text.rotation.cos());
                let mid = [
                    text.origin[0] + dx * cap / 2.0,
                    text.origin[1] + dy * cap / 2.0,
                ];
                let mut mtext = MText::new();
                mtext.value = text.text.trim_end().to_string();
                mtext.insertion_point = world3(mid);
                mtext.height = cap * scale;
                mtext.rectangle_width = 0.0;
                mtext.rotation = text.rotation + underlay.rotation;
                mtext.style = style;
                mtext.attachment_point = AttachmentPoint::MiddleLeft;
                // Text goes to the text layer whatever PDF layer holds it.
                let layer = naming.layer(None, Kind::Text);
                mtext.common.color = color_on(&layer, color_of(text.color));
                mtext.common.layer = layer;
                result.entities.push(EntityType::MText(Box::new(mtext)));
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
                    image.origin[0]
                        + (image.u[0] * image.width as f64 + image.v[0] * image.height as f64)
                            / 2.0,
                    image.origin[1]
                        + (image.u[1] * image.width as f64 + image.v[1] * image.height as f64)
                            / 2.0,
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

    // Strokes that trace circles, arcs and ellipses become those: each
    // stroke first, then the strokes the rest join into; then (when asked)
    // dashes read as one dashed line.
    let mut fit = |strokes: Vec<Stroke>| -> Vec<Stroke> {
        strokes
            .into_iter()
            .filter_map(|stroke| {
                let points: Vec<[f64; 2]> = stroke
                    .points
                    .iter()
                    .map(|p| {
                        let w = world2(*p);
                        [w.x, w.y]
                    })
                    .collect();
                let bulges: Vec<f64> = stroke.bulges.iter().map(|b| b * turn).collect();
                match curve_of(&points, &bulges, stroke.closed, z) {
                    Some(mut curve) => {
                        let common = curve.common_mut();
                        (common.layer, common.color, common.line_weight) =
                            (stroke.layer, stroke.color, stroke.weight);
                        curves.push((stroke.order, curve));
                        None
                    }
                    None => Some(stroke),
                }
            })
            .collect()
    };
    let strokes = fit(strokes);
    let strokes = fit(join_strokes(strokes, settings.join));
    let strokes: Vec<(Stroke, Option<f64>)> = if settings.linetypes {
        infer_dashes(strokes)
    } else {
        strokes.into_iter().map(|s| (s, None)).collect()
    };
    for (stroke, dash) in strokes {
        // A lone arc is an ARC.
        if !stroke.closed && stroke.points.len() == 2 && stroke.bulges[0] != 0.0 {
            let (a, b) = (world2(stroke.points[0]), world2(stroke.points[1]));
            let bulge = stroke.bulges[0] * turn;
            if let Some(ba) =
                crate::entities::common::BulgeArc::from_bulge([a.x, a.y], [b.x, b.y], bulge)
            {
                let (start, end) = if bulge > 0.0 {
                    (ba.start_angle, ba.end_angle)
                } else {
                    (ba.end_angle, ba.start_angle)
                };
                let tau = std::f64::consts::TAU;
                let mut arc = Arc::from_center_radius_angles(
                    Vector3::new(ba.center[0], ba.center[1], z),
                    ba.radius,
                    start.rem_euclid(tau),
                    end.rem_euclid(tau),
                );
                (arc.common.layer, arc.common.color, arc.common.line_weight) =
                    (stroke.layer, stroke.color, stroke.weight);
                curves.push((stroke.order, EntityType::Arc(arc)));
                continue;
            }
        }
        let mut pl = LwPolyline::from_points(stroke.points.iter().map(|p| world2(*p)).collect());
        for (vertex, bulge) in pl.vertices.iter_mut().zip(&stroke.bulges) {
            vertex.bulge = bulge * turn;
        }
        pl.is_closed = stroke.closed;
        pl.elevation = z;
        (pl.common.layer, pl.common.color, pl.common.line_weight) =
            (stroke.layer, stroke.color, stroke.weight);
        if let Some(dash) = dash {
            pl.common.linetype = DASH_LINETYPE.to_string();
            pl.common.linetype_scale = dash * scale;
            result.dashed = true;
        }
        curves.push((stroke.order, EntityType::LwPolyline(pl)));
    }
    // In drawing order (the sort is stable: a subpath's pieces keep theirs).
    curves.sort_by_key(|(order, _)| *order);
    result.entities.extend(curves.into_iter().map(|(_, entity)| entity));
    result.layers = layer_colors;
    layer_colours_by_majority(&mut result);
    for image in &result.images {
        if !result.layers.iter().any(|(n, _)| *n == image.layer) {
            result.layers.push((image.layer.clone(), Color::Index(7)));
        }
    }
    result
}

/// Each layer takes the colour most of its objects have (the first such
/// colour on a tie); those objects then follow the layer.
fn layer_colours_by_majority(result: &mut ImportResult) {
    for (layer, layer_color) in &mut result.layers {
        let mut counts: Vec<(Color, usize)> = Vec::new();
        for entity in &result.entities {
            let common = entity.common();
            if common.layer != *layer {
                continue;
            }
            match counts.iter_mut().find(|(c, _)| *c == common.color) {
                Some((_, n)) => *n += 1,
                None => counts.push((common.color, 1)),
            }
        }
        // `max_by_key` keeps the last maximum: search from the back.
        if let Some((color, _)) = counts.iter().rev().max_by_key(|(_, n)| *n) {
            *layer_color = *color;
        }
        for entity in &mut result.entities {
            let common = entity.common_mut();
            if common.layer == *layer && common.color == *layer_color {
                common.color = Color::ByLayer;
            }
        }
    }
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
                [
                    a[0] + (b[0] - a[0]) * 2.0 / 3.0,
                    a[1] + (b[1] - a[1]) * 2.0 / 3.0,
                ],
                *b,
            ),
        };
        let pts = if i == 0 {
            vec![p0, p1, p2, p3]
        } else {
            vec![p1, p2, p3]
        };
        controls.extend(pts.into_iter().map(world3));
    }
    let spans = sp.segments.len();
    let mut knots = vec![0.0; 4];
    for k in 1..spans {
        knots.extend([k as f64; 3]);
    }
    knots.extend([spans as f64; 4]);
    // Planar, and linear when its control points lie on one line.
    let (a, b) = (controls[0], controls[controls.len() - 1]);
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let span = dx.hypot(dy);
    spline.flags.linear = span > 1e-12
        && controls
            .iter()
            .all(|p| ((p.x - a.x) * dy - (p.y - a.y) * dx).abs() <= 1e-9 * span);
    spline.flags.planar = true;
    spline.control_points = controls;
    spline.weights = Vec::new();
    spline.knots = knots;
    spline.flags.closed = sp.closed;
    spline
}

/// A fill path as objects: the filled areas (each with whether it is a
/// four-cornered SOLID, which takes the stroke width) and the lines and
/// curves that bound no area.
struct FillOut {
    fills: Vec<(EntityType, bool)>,
    lines: Vec<EntityType>,
}

/// A stretch of a subpath between its curves that are no circular arcs:
/// a run of straight segments, or one circular arc.
struct FillPiece {
    points: Vec<[f64; 2]>,
    bulges: Vec<f64>,
    /// Closed by a close operator that draws a line back to the subpath's
    /// start (the last stretch of its subpath).
    closed: bool,
    /// The subpath it comes from when that has other curves; `usize::MAX`
    /// for every subpath without them.
    group: usize,
}

impl FillPiece {
    fn straight(&self) -> bool {
        self.bulges.iter().all(|b| *b == 0.0)
    }
    /// The loop it bounds: its points without a last one back on the first.
    fn ring(&self) -> (Vec<[f64; 2]>, Vec<f64>) {
        let (mut points, mut bulges) = (self.points.clone(), self.bulges.clone());
        if points.len() > 2 && same_point(points[0], points[points.len() - 1]) {
            points.pop();
            bulges.pop();
        }
        (points, bulges)
    }
}

/// A fill path read as the reference reads it. Each subpath is cut at its
/// curves that are no circular arcs (each such curve its own spline) and at
/// its circular arcs (each its own stretch); a close operator that draws a
/// line back to the start closes the last stretch on itself instead. A lone
/// subpath without such curves that ends on its start is one loop through
/// all its stretches. Otherwise the stretches of each subpath with such
/// curves, and those of all subpaths without them, are read together: a
/// closed last stretch of three straight corners is a triangle SOLID alone;
/// one stretch is a line (two points: closed when closed, or when it is a
/// whole subpath), a SOLID (three or four straight corners) or a hatch;
/// more stretches are one hatch when one of them has three or more points
/// or an arc (SOLIDs each when every one is a straight three- or
/// four-cornered subpath), or when there are three or more; two two-point
/// stretches stay lines. A SOLID of four corners, or of three not closed by
/// the operator, takes the stroke width; a straight loop on one line is a
/// closed line. A fill reaching out of its clip is cut to it as a hatch.
fn fill_entities(
    path: &PdfPath,
    hatches: bool,
    turn: f64,
    world2: &dyn Fn([f64; 2]) -> Vector2,
    world3: &dyn Fn([f64; 2]) -> Vector3,
    z: f64,
) -> FillOut {
    let mut pieces: Vec<FillPiece> = Vec::new();
    let mut curves: Vec<Segment> = Vec::new();
    let mut lone = path.subpaths.len() == 1;
    for (k, sp) in path.subpaths.iter().enumerate() {
        let close = path.closes.get(k).copied().unwrap_or(SubClose {
            op: sp.closed,
            line: false,
        });
        let segments = match close.line {
            true => &sp.segments[..sp.segments.len() - 1],
            false => &sp.segments[..],
        };
        let curved = |s: &Segment| matches!(s, Segment::Cubic(..)) && arc_bulge(s).is_none();
        let has_curve = segments.iter().any(curved);
        lone &= !has_curve && sp.closed && !close.line;
        let group = if has_curve { k } else { usize::MAX };
        let mut run: Vec<[f64; 2]> = vec![sp.segments[0].start()];
        let flush = |run: &mut Vec<[f64; 2]>, pieces: &mut Vec<FillPiece>| {
            let points = std::mem::take(run);
            let n = points.len();
            if n < 2 {
                return;
            }
            pieces.push(FillPiece {
                points,
                bulges: vec![0.0; n],
                closed: false,
                group,
            });
        };
        for seg in segments {
            match seg {
                Segment::Line(_, b) => run.push(*b),
                Segment::Cubic(a, .., d) => {
                    flush(&mut run, &mut pieces);
                    match arc_bulge(seg) {
                        Some(bulge) => pieces.push(FillPiece {
                            points: vec![*a, *d],
                            bulges: vec![bulge, 0.0],
                            closed: false,
                            group,
                        }),
                        None => curves.push(seg.clone()),
                    }
                    run.push(*d);
                }
            }
        }
        let before = pieces.len();
        flush(&mut run, &mut pieces);
        if close.line && pieces.len() > before {
            if let Some(last) = pieces.last_mut() {
                last.closed = true;
            }
        }
    }
    let line = |points: &[[f64; 2]], bulges: &[f64], closed: bool| {
        let mut pl = LwPolyline::from_points(points.iter().map(|p| world2(*p)).collect());
        for (vertex, bulge) in pl.vertices.iter_mut().zip(bulges) {
            vertex.bulge = bulge * turn;
        }
        pl.is_closed = closed;
        pl.elevation = z;
        EntityType::LwPolyline(pl)
    };
    let mut lines: Vec<EntityType> = curves
        .iter()
        .map(|seg| {
            let one = SubPath {
                segments: vec![seg.clone()],
                closed: false,
            };
            EntityType::Spline(spline_of(&one, world3))
        })
        .collect();
    // A loop bounding no area (its points on one line).
    let flat = |points: &[[f64; 2]]| {
        let d = |p: &[f64; 2]| (p[0] - points[0][0]).hypot(p[1] - points[0][1]);
        let far = points.iter().copied().max_by(|a, b| d(a).total_cmp(&d(b)));
        let far = far.unwrap_or(points[0]);
        let (dx, dy) = (far[0] - points[0][0], far[1] - points[0][1]);
        let span = dx.hypot(dy).max(1e-9);
        points
            .iter()
            .all(|p| ((p[0] - points[0][0]) * dy - (p[1] - points[0][1]) * dx).abs() <= 1e-9 * span)
    };
    // A straight loop of three or four corners as a SOLID, with whether it
    // takes the stroke width.
    let solid = |points: &[[f64; 2]], closed: bool| -> Option<(EntityType, bool)> {
        match points.len() {
            3 => Some((
                EntityType::Solid(Solid::triangle(
                    world3(points[0]),
                    world3(points[1]),
                    world3(points[2]),
                )),
                !closed,
            )),
            // SOLID corners run 1-2-4-3 (the second edge crosses).
            4 if !hatches => Some((
                EntityType::Solid(Solid::new(
                    world3(points[0]),
                    world3(points[1]),
                    world3(points[3]),
                    world3(points[2]),
                )),
                true,
            )),
            _ => None,
        }
    };
    let hatch = |rings: &[(Vec<[f64; 2]>, Vec<f64>)]| -> Option<EntityType> {
        let mut hatch = Hatch::solid();
        for (points, bulges) in rings {
            if points.len() < 2 {
                continue;
            }
            let mut edge = PolylineEdge::new(Vec::new(), true);
            for (p, bulge) in points.iter().zip(bulges) {
                edge.add_vertex(world2(*p), bulge * turn);
            }
            let mut boundary = BoundaryPath::external();
            boundary.flags.set_polyline(true);
            boundary.edges.push(BoundaryEdge::Polyline(edge));
            hatch.add_path(boundary);
        }
        hatch.elevation = z;
        (!hatch.paths.is_empty()).then(|| EntityType::Hatch(Box::new(hatch)))
    };
    // Straight loops cut to a clip they reach out of make a hatch.
    let clipped = |rings: &[(Vec<[f64; 2]>, Vec<f64>)]| -> Option<Vec<(EntityType, bool)>> {
        let clip = path.clip.as_ref()?;
        if rings.iter().any(|(_, b)| b.iter().any(|b| *b != 0.0)) {
            return None;
        }
        let cut: Vec<(Vec<[f64; 2]>, Vec<f64>)> = rings
            .iter()
            .map(|(points, _)| crate::scene::model::pdf_vector::clip_to_convex(points, clip))
            .filter(|points| points.len() > 2)
            .map(|points| {
                let n = points.len();
                (points, vec![0.0; n])
            })
            .collect();
        Some(hatch(&cut).into_iter().map(|h| (h, false)).collect())
    };
    let mut fills: Vec<(EntityType, bool)> = Vec::new();
    // One loop: a line, a SOLID or a hatch. A two-point loop is a closed
    // line when `shut`.
    let single = |(points, bulges): (Vec<[f64; 2]>, Vec<f64>),
                  closed: bool,
                  shut: bool,
                  fills: &mut Vec<(EntityType, bool)>,
                  lines: &mut Vec<EntityType>| {
        if let Some(out) = clipped(std::slice::from_ref(&(points.clone(), bulges.clone()))) {
            fills.extend(out);
            return;
        }
        let straight = bulges.iter().all(|b| *b == 0.0);
        if points.len() == 2 && straight {
            lines.push(line(&points, &bulges, shut));
        } else if straight && flat(&points) {
            lines.push(line(&points, &bulges, true));
        } else if let Some(s) = straight.then(|| solid(&points, closed)).flatten() {
            fills.push(s);
        } else {
            fills.extend(hatch(&[(points, bulges)]).map(|h| (h, false)));
        }
    };
    if lone && !pieces.is_empty() {
        let mut points: Vec<[f64; 2]> = Vec::new();
        let mut bulges: Vec<f64> = Vec::new();
        for piece in &pieces {
            points.extend(&piece.points);
            bulges.extend(&piece.bulges);
        }
        let closed = pieces.iter().any(|p| p.closed);
        let ring = FillPiece {
            points,
            bulges,
            closed,
            group: 0,
        }
        .ring();
        single(ring, closed, closed, &mut fills, &mut lines);
        return FillOut { fills, lines };
    }
    let mut groups: Vec<usize> = Vec::new();
    for piece in &pieces {
        if !groups.contains(&piece.group) {
            groups.push(piece.group);
        }
    }
    let four = |p: &FillPiece| p.straight() && (3..=4).contains(&p.ring().0.len());
    for group in groups {
        let all: Vec<&FillPiece> = pieces.iter().filter(|p| p.group == group).collect();
        // A subpath's closed last stretch of three straight corners.
        let alone = |p: &FillPiece| {
            group != usize::MAX && p.closed && p.straight() && p.ring().0.len() == 3
        };
        for p in all.iter().filter(|p| alone(p)) {
            single(p.ring(), p.closed, p.closed, &mut fills, &mut lines);
        }
        let set: Vec<&FillPiece> = all.into_iter().filter(|p| !alone(p)).collect();
        match set.as_slice() {
            [] => {}
            // A whole subpath of two points closes.
            [p] => {
                let shut = p.closed || group == usize::MAX;
                single(p.ring(), p.closed, shut, &mut fills, &mut lines)
            }
            set => {
                let wide = set.iter().any(|p| p.points.len() > 2 || !p.straight());
                if !wide && set.len() < 3 {
                    for p in set {
                        lines.push(line(&p.points, &p.bulges, p.closed));
                    }
                    continue;
                }
                let rings: Vec<(Vec<[f64; 2]>, Vec<f64>)> = set.iter().map(|p| p.ring()).collect();
                let solids: Option<Vec<(EntityType, bool)>> = set
                    .iter()
                    .zip(&rings)
                    .map(|(p, (points, _))| {
                        (group == usize::MAX && four(p))
                            .then(|| solid(points, p.closed))
                            .flatten()
                    })
                    .collect();
                if let Some(out) = clipped(&rings) {
                    fills.extend(out);
                } else if let Some(solids) = solids {
                    fills.extend(solids);
                } else {
                    fills.extend(hatch(&rings).map(|h| (h, false)));
                }
            }
        }
    }
    FillOut { fills, lines }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["PDFIMPORT", "-PDFIMPORT"]
});
