// PDFATTACH / -PDFATTACH — attach a PDF page as an underlay; DWFATTACH /
// DGNATTACH (and their - forms) attach a DWF sheet or a DGN model.
//
//   Path to PDF file to attach:                (-PDFATTACH only)
//   Enter page number or [?] <1>:              (DWF: name of sheet, DGN: model)
//   Specify conversion units [Master/Sub] <Master>:   (DGN only)
//   Specify insertion point:
//   Base image size: Width: 7.8740, Height: 3.9369, Meters
//   Specify scale factor or [Unit] <1>:
//   Specify rotation <last>:
//
// The definition (one per file and page, named "<file> - <page>" in the
// ACAD_PDFDEFINITIONS, ACAD_DWFDEFINITIONS or ACAD_DGNDEFINITIONS dictionary) is created with the underlay when it is
// placed, so a cancelled attach leaves nothing and undo removes both.

use std::sync::Mutex;

use codec::entities::{Underlay, UnderlayDisplayFlags, UnderlayType};
use codec::objects::{Dictionary, ObjectType, UnderlayDefinition};
use codec::types::{Handle, Vector3};
use codec::{CadDocument, EntityType};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, WorkingPlane};
use crate::scene::model::wire_model::WireModel;
use crate::modules::IconKind;

pub const ICON: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/underlay_layers.svg"));

/// Units the Unit option offers: keyword, the name the prompt shows.
const UNITS: [(&str, &str); 9] = [
    ("MM", "Millimeters"),
    ("Centimeter", "Centimeters"),
    ("Meter", "Meters"),
    ("Kilometer", "Kilometers"),
    ("Inch", "Inches"),
    ("Foot", "Feet"),
    ("Yard", "Yards"),
    ("MILe", "Miles"),
    ("Unitless", "Unitless"),
];

/// The unit last chosen with the Unit option; it stays the default.
static LAST_UNIT: Mutex<Option<usize>> = Mutex::new(None);

/// The rotation (degrees) of the last underlay attached; the next attach
/// offers it.
static LAST_ROTATION: Mutex<f64> = Mutex::new(0.0);

fn last_rotation() -> f64 {
    LAST_ROTATION.lock().map(|r| *r).unwrap_or(0.0)
}

/// The Unit option's index for the drawing's INSUNITS.
fn unit_for_insunits(insunits: i16) -> usize {
    match insunits {
        4 => 0,
        5 => 1,
        6 => 2,
        7 => 3,
        1 => 4,
        2 => 5,
        10 => 6,
        3 => 7,
        _ => 8,
    }
}

fn unit_from_keyword(text: &str) -> Option<usize> {
    let t = text.trim().to_ascii_uppercase();
    let short = match t.as_str() {
        "MM" => Some(0),
        "C" => Some(1),
        "M" => Some(2),
        "K" => Some(3),
        "I" => Some(4),
        "F" => Some(5),
        "Y" => Some(6),
        "MIL" => Some(7),
        "U" => Some(8),
        _ => None,
    };
    short.or_else(|| {
        UNITS.iter().position(|(keyword, _)| {
            let k = keyword.to_ascii_uppercase();
            t.len() >= 2 && k.starts_with(&t)
        })
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Path,
    Page,
    ListPages,
    Conversion,
    Insertion,
    Scale,
    Unit,
    Rotation,
}

pub struct PdfAttachCommand {
    kind: UnderlayType,
    step: Step,
    /// The file read for the page (absolute or registered source).
    path: String,
    /// The path the definition stores (full, relative or file name only).
    stored_path: Option<String>,
    /// Pages after the first, attached at the same point (dialog selection).
    extra_pages: Vec<String>,
    /// Scale and rotation fixed in the dialog: their prompts are skipped.
    preset_scale: Option<f64>,
    preset_rotation: Option<f64>,
    page: String,
    page_count: usize,
    /// DWF sheet or DGN model names of the file.
    items: Vec<String>,
    /// The page (PDF inches from its corner) or sheet rectangle.
    rect: [f64; 4],
    /// DGN sub units per master unit (the Sub conversion's scale).
    sub_per_master: f64,
    /// Scale the scale prompt offers.
    default_scale: f64,
    insertion: DVec3,
    scale: f64,
    unit: usize,
    plane: WorkingPlane,
}

impl PdfAttachCommand {
    /// `-PDFATTACH`: starts by asking for the file.
    pub fn new(insunits: i16) -> Self {
        Self::for_kind(UnderlayType::Pdf, insunits)
    }

    /// `-DWFATTACH` / `-DGNATTACH` (or `-PDFATTACH`): asks for the file.
    pub fn for_kind(kind: UnderlayType, insunits: i16) -> Self {
        let unit = LAST_UNIT
            .lock()
            .ok()
            .and_then(|last| *last)
            .unwrap_or_else(|| unit_for_insunits(insunits));
        Self {
            kind,
            step: Step::Path,
            path: String::new(),
            stored_path: None,
            extra_pages: Vec::new(),
            preset_scale: None,
            preset_rotation: None,
            page: "1".to_string(),
            page_count: 0,
            items: Vec::new(),
            rect: [0.0; 4],
            sub_per_master: 1.0,
            default_scale: 1.0,
            insertion: DVec3::ZERO,
            scale: 1.0,
            unit,
            plane: WorkingPlane::default(),
        }
    }

    /// DWFATTACH / DGNATTACH / PDFATTACH with the file chosen: starts at the
    /// page, sheet or model prompt.
    pub fn with_kind_file(kind: UnderlayType, path: &str, insunits: i16) -> Self {
        let mut command = Self::for_kind(kind, insunits);
        command.open(path);
        command
    }

    /// Reads the file's pages or sheet / model names; false when it cannot.
    fn open(&mut self, path: &str) -> bool {
        let found = match self.kind {
            UnderlayType::Pdf => crate::scene::model::pdf_raster::page_count(path).filter(|n| *n > 0).map(|n| {
                self.page_count = n;
            }),
            kind => crate::scene::model::underlay_vector::item_names(kind, path)
                .filter(|names| !names.is_empty())
                .map(|names| {
                    self.page = names[0].clone();
                    self.items = names;
                }),
        };
        if found.is_some() {
            self.path = path.to_string();
            self.step = Step::Page;
        }
        found.is_some()
    }

    fn page_rect(&self, page: &str) -> [f64; 4] {
        match self.kind {
            UnderlayType::Pdf => crate::scene::model::pdf_raster::page_size_inches(&self.path, page)
                .map(|(w, h)| [0.0, 0.0, w, h])
                .unwrap_or([0.0; 4]),
            kind => crate::scene::model::underlay_vector::sheet(kind, &self.path, page)
                .map(|s| s.rect)
                .unwrap_or([0.0; 4]),
        }
    }

    /// The DWF / DGN (or PDF) Attach dialog: the sheet or model chosen and,
    /// for DGN, whether sub units were chosen (their scale is then offered).
    #[allow(clippy::too_many_arguments)]
    pub fn from_kind_dialog(
        kind: UnderlayType,
        read_path: &str,
        stored_path: &str,
        pages: &[String],
        scale: Option<f64>,
        rotation_deg: Option<f64>,
        sub_units: bool,
        insunits: i16,
    ) -> Self {
        let mut command = Self::with_kind_file(kind, read_path, insunits);
        command.stored_path = Some(stored_path.to_string());
        if let Some((first, rest)) = pages.split_first() {
            command.page = first.clone();
            command.extra_pages = rest.to_vec();
        }
        command.rect = command.page_rect(&command.page);
        if kind == UnderlayType::Dgn && sub_units {
            let sub = crate::scene::model::underlay_vector::sheet(kind, read_path, &command.page)
                .map(|s| s.sub_per_master)
                .filter(|v| *v > 0.0)
                .unwrap_or(1.0);
            command.default_scale = 1.0 / sub;
            command.scale = command.default_scale;
        }
        command.preset_scale = scale;
        if let Some(s) = scale {
            command.scale = s;
        }
        command.preset_rotation = rotation_deg;
        command.step = Step::Insertion;
        command
    }

    /// After the insertion point or the scale: the next prompt, or the
    /// attach itself when the dialog fixed what is left.
    fn after_scale(&mut self) -> CmdResult {
        match self.preset_rotation {
            Some(deg) => self.commit(deg.to_radians()),
            None => {
                self.step = Step::Rotation;
                CmdResult::NeedPoint
            }
        }
    }

    fn base_image_size(&self) -> String {
        format!(
            "Base image size: Width: {:.4}, Height: {:.4}, {}",
            self.rect[2] - self.rect[0],
            self.rect[3] - self.rect[1],
            UNITS[self.unit].1
        )
    }

    fn accept_path(&mut self, text: &str) -> CmdResult {
        let mut path = text.trim().trim_matches('"').to_string();
        if path.is_empty() {
            return CmdResult::NeedPoint;
        }
        if std::path::Path::new(&path).extension().is_none() {
            path.push_str(match self.kind {
                UnderlayType::Pdf => ".pdf",
                UnderlayType::Dwf => ".dwf",
                UnderlayType::Dgn => ".dgn",
            });
        }
        match self.open(&path) {
            true => CmdResult::NeedPoint,
            false => CmdResult::ReportError(format!(
                "{} not found.",
                crate::entities::underlay::display_path(&path)
            )),
        }
    }

    fn accept_page(&mut self, text: &str) -> CmdResult {
        let text = text.trim();
        if text == "?" {
            self.step = Step::ListPages;
            return CmdResult::NeedPoint;
        }
        if self.kind != UnderlayType::Pdf {
            let name = if text.is_empty() { self.items[0].clone() } else { text.to_string() };
            let Some(found) = self.items.iter().find(|n| n.eq_ignore_ascii_case(&name)) else {
                // ponytail: the PDF wording; the reference's message for an
                // unknown sheet / model name was not measured.
                return CmdResult::ReportError(format!(
                    "There is no {name} in {}.",
                    crate::entities::underlay::display_path(&self.path)
                ));
            };
            self.page = found.clone();
            self.rect = self.page_rect(&self.page);
            if self.kind == UnderlayType::Dgn {
                self.sub_per_master = crate::scene::model::underlay_vector::sheet(self.kind, &self.path, &self.page)
                    .map(|s| s.sub_per_master)
                    .filter(|v| *v > 0.0)
                    .unwrap_or(1.0);
                self.step = Step::Conversion;
            } else {
                self.step = Step::Insertion;
            }
            return CmdResult::NeedPoint;
        }
        let page = if text.is_empty() { "1" } else { text };
        match page.parse::<usize>() {
            Ok(n) if n >= 1 && n <= self.page_count => {
                self.page = n.to_string();
                self.rect = self.page_rect(&self.page);
                self.step = Step::Insertion;
                CmdResult::NeedPoint
            }
            _ => CmdResult::ReportError(format!(
                "There is no {page} in {}.",
                crate::entities::underlay::display_path(&self.path)
            )),
        }
    }

    /// DGN: Master keeps the model's master units, Sub offers the scale that
    /// turns sub units into master units.
    fn accept_conversion(&mut self, text: &str) -> CmdResult {
        let t = text.trim().to_ascii_uppercase();
        let sub = match t.as_str() {
            "" => false,
            _ if "MASTER".starts_with(&t) => false,
            _ if "SUB".starts_with(&t) => true,
            _ => return CmdResult::ReportError("Invalid option keyword.".to_string()),
        };
        self.default_scale = if sub { 1.0 / self.sub_per_master } else { 1.0 };
        self.scale = self.default_scale;
        self.step = Step::Insertion;
        CmdResult::NeedPoint
    }

    fn list_pages(&mut self, pattern: &str) -> CmdResult {
        let pattern = if pattern.trim().is_empty() { "*" } else { pattern.trim() };
        let names: Vec<String> = match self.kind {
            UnderlayType::Pdf => (1..=self.page_count).map(|n| n.to_string()).collect(),
            _ => self.items.clone(),
        };
        let lines: Vec<String> = names
            .into_iter()
            .filter(|n| crate::io::xref_model::wildcard_match(n, pattern))
            .collect();
        self.step = Step::Page;
        CmdResult::ReportMeasurement(lines.join("\n"))
    }

    fn accept_scale(&mut self, text: &str) -> CmdResult {
        let text = text.trim();
        if text.is_empty() {
            return self.after_scale();
        }
        if text.eq_ignore_ascii_case("U") || text.eq_ignore_ascii_case("UNIT") {
            self.step = Step::Unit;
            return CmdResult::NeedPoint;
        }
        match crate::entities::common::parse_f64(text) {
            Some(v) if v > 0.0 => {
                self.scale = v;
                self.after_scale()
            }
            Some(_) => CmdResult::ReportError("Value must be positive and nonzero.".to_string()),
            None => CmdResult::ReportError("Requires numeric value or option keyword.".to_string()),
        }
    }

    fn accept_unit(&mut self, text: &str) -> CmdResult {
        let unit = if text.trim().is_empty() {
            Some(self.unit)
        } else {
            unit_from_keyword(text)
        };
        let Some(unit) = unit else {
            return CmdResult::ReportError("Invalid option keyword.".to_string());
        };
        self.unit = unit;
        if let Ok(mut last) = LAST_UNIT.lock() {
            *last = Some(unit);
        }
        self.step = Step::Scale;
        CmdResult::ReportMeasurement(self.base_image_size())
    }

    fn accept_rotation(&mut self, text: &str) -> CmdResult {
        let text = text.trim();
        let degrees = if text.is_empty() {
            last_rotation()
        } else {
            match crate::entities::common::parse_f64(text) {
                Some(v) => v,
                None => {
                    return CmdResult::ReportError(
                        "Requires numeric angle or second point.".to_string(),
                    )
                }
            }
        };
        if let Ok(mut last) = LAST_ROTATION.lock() {
            *last = degrees;
        }
        self.commit(degrees.to_radians())
    }

    fn commit(&mut self, rotation: f64) -> CmdResult {
        let pages: Vec<String> = std::iter::once(self.page.clone())
            .chain(self.extra_pages.iter().cloned())
            .collect();
        let placed = underlays_for_pages(
            self.kind,
            &pages,
            self.plane.to_local(self.insertion),
            self.scale,
            rotation,
        )
        .into_iter()
        .map(|(page, underlay)| (page, self.plane.place_entity(underlay)))
        .collect();
        CmdResult::AttachPdfPages {
            kind: self.kind,
            path: self.stored_path.clone().unwrap_or_else(|| self.path.clone()),
            pages: placed,
        }
    }

    fn frame_at(&self, pt: DVec3) -> Vec<[f64; 3]> {
        let at = |x: f64, y: f64| pt + self.plane.vector_to_world(DVec3::new(x * self.scale, y * self.scale, 0.0));
        let [x0, y0, x1, y1] = self.rect;
        let corners = [at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1), at(x0, y0)];
        corners.iter().map(|p| p.to_array()).collect()
    }
}

impl CadCommand for PdfAttachCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        match self.kind {
            UnderlayType::Pdf => "PDFATTACH",
            UnderlayType::Dwf => "DWFATTACH",
            UnderlayType::Dgn => "DGNATTACH",
        }
    }

    fn prompt(&self) -> String {
        let item = if self.kind == UnderlayType::Dwf { "sheet" } else { "model" };
        match self.step {
            Step::Path => format!(
                "Path to {} file to attach:",
                match self.kind {
                    UnderlayType::Pdf => "PDF",
                    UnderlayType::Dwf => "DWF",
                    UnderlayType::Dgn => "DGN",
                }
            ),
            Step::Page if self.kind == UnderlayType::Pdf => "Enter page number or [?] <1>:".to_string(),
            Step::Page => format!("Enter name of {item} or [?] <{}>:", self.items.first().map_or("", |s| s)),
            Step::ListPages if self.kind == UnderlayType::Pdf => "Enter page(s) to list <*>:".to_string(),
            Step::ListPages => format!("Enter {item}(s) to list <*>:"),
            Step::Conversion => "Specify conversion units [Master/Sub] <Master>:".to_string(),
            Step::Insertion => "Specify insertion point:".to_string(),
            Step::Scale => format!("Specify scale factor or [Unit] <{}>:", self.default_scale),
            Step::Unit => format!(
                "Enter unit [MM/Centimeter/Meter/Kilometer/Inch/Foot/Yard/MILe/Unitless] <{}>:",
                UNITS[self.unit].0
            ),
            Step::Rotation => format!("Specify rotation <{}>:", last_rotation()),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Page => vec![CmdOption::new("?", "?")],
            Step::Conversion => vec![CmdOption::new("Master", "M"), CmdOption::new("Sub", "S")],
            Step::Scale => vec![CmdOption::new("Unit", "U")],
            Step::Unit => vec![
                CmdOption::new("MM", "MM"),
                CmdOption::new("Centimeter", "C"),
                CmdOption::new("Meter", "M"),
                CmdOption::new("Kilometer", "K"),
                CmdOption::new("Inch", "I"),
                CmdOption::new("Foot", "F"),
                CmdOption::new("Yard", "Y"),
                CmdOption::new("MILe", "MIL"),
                CmdOption::new("Unitless", "U"),
            ],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Path => InputKind::FreeText,
            Step::Insertion => InputKind::Point,
            _ => InputKind::SingleToken,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.step == Step::Conversion {
            return CmdResult::ReportError("Invalid option keyword.".to_string());
        }
        if self.step != Step::Insertion {
            return CmdResult::NeedPoint;
        }
        self.insertion = pt;
        if self.preset_scale.is_some() {
            return self.after_scale();
        }
        self.step = Step::Scale;
        CmdResult::ReportMeasurement(self.base_image_size())
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        Some(match self.step {
            Step::Path => self.accept_path(text),
            Step::Page => self.accept_page(text),
            Step::ListPages => self.list_pages(text),
            Step::Conversion => self.accept_conversion(text),
            Step::Insertion => return None,
            Step::Scale => self.accept_scale(text),
            Step::Unit => self.accept_unit(text),
            Step::Rotation => self.accept_rotation(text),
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Path => CmdResult::Cancel,
            Step::Page => self.accept_page(""),
            Step::ListPages => self.list_pages("*"),
            Step::Conversion => self.accept_conversion(""),
            Step::Insertion => {
                CmdResult::ReportError("Point or option keyword required.".to_string())
            }
            Step::Scale => self.accept_scale(""),
            Step::Unit => self.accept_unit(""),
            Step::Rotation => self.accept_rotation(""),
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if self.step != Step::Insertion || self.rect[2] <= self.rect[0] {
            return None;
        }
        Some(WireModel::solid_f64(
            "pdf_attach_frame".into(),
            self.frame_at(pt),
            WireModel::CYAN,
            false,
        ))
    }
}

/// Underlays for `pages` of a file, all at `insertion` with the same scale
/// and rotation (the reference stacks the chosen pages there).
pub fn underlays_for_pages(
    kind: UnderlayType,
    pages: &[String],
    insertion: DVec3,
    scale: f64,
    rotation: f64,
) -> Vec<(String, EntityType)> {
    pages
        .iter()
        .map(|page| {
            let mut underlay = Underlay::new(kind);
            underlay.insertion_point = Vector3::new(insertion.x, insertion.y, insertion.z);
            underlay.set_scale(scale);
            underlay.rotation = rotation;
            // On, clipped by its boundary and colour-adjusted for the
            // background, as the reference creates an underlay.
            underlay.flags = UnderlayDisplayFlags::ON
                | UnderlayDisplayFlags::CLIPPING
                | UnderlayDisplayFlags::ADJUST_FOR_BACKGROUND;
            if kind != UnderlayType::Pdf {
                // The reference creates DWF and DGN underlays at contrast 75,
                // fade 25.
                underlay.contrast = 75;
                underlay.fade = 25;
            }
            (page.clone(), EntityType::Underlay(underlay))
        })
        .collect()
}

/// The kind's definitions dictionary (`ACAD_PDFDEFINITIONS` …) under the
/// named-objects root, created when the drawing has none.
fn ensure_definitions_dictionary(document: &mut CadDocument, kind: UnderlayType) -> Option<Handle> {
    let name = match kind {
        UnderlayType::Pdf => "ACAD_PDFDEFINITIONS",
        UnderlayType::Dwf => "ACAD_DWFDEFINITIONS",
        UnderlayType::Dgn => "ACAD_DGNDEFINITIONS",
    };
    let root = document.header.named_objects_dict_handle;
    let Some(ObjectType::Dictionary(root_dictionary)) = document.objects.get(&root) else {
        return None;
    };
    if let Some(existing) = root_dictionary.get(name) {
        if matches!(document.objects.get(&existing), Some(ObjectType::Dictionary(_))) {
            return Some(existing);
        }
    }
    let handle = document.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = handle;
    dictionary.owner = root;
    document.objects.insert(handle, ObjectType::Dictionary(dictionary));
    if let Some(ObjectType::Dictionary(root_dictionary)) = document.objects.get_mut(&root) {
        root_dictionary.add_entry(name, handle);
    }
    Some(handle)
}

fn same_path(a: &str, b: &str) -> bool {
    let norm = |p: &str| p.replace('\\', "/").to_lowercase();
    norm(a) == norm(b)
}

/// The definition for a file and page / sheet / model of the kind: the
/// existing one, or a new one registered in its dictionary as
/// "<file> - <page>".
pub fn ensure_underlay_definition(document: &mut CadDocument, kind: UnderlayType, path: &str, page: &str) -> Handle {
    let existing = document.objects.iter().find_map(|(handle, object)| match object {
        ObjectType::UnderlayDefinition(def)
            if def.underlay_type == kind
                && same_path(&def.file_path, path)
                && crate::entities::underlay::page_of(def) == page =>
        {
            Some(*handle)
        }
        _ => None,
    });
    if let Some(handle) = existing {
        return handle;
    }
    let handle = document.allocate_handle();
    let mut definition = match kind {
        UnderlayType::Pdf => UnderlayDefinition::pdf(path, page),
        UnderlayType::Dwf => UnderlayDefinition::dwf(path, page),
        UnderlayType::Dgn => UnderlayDefinition::dgn(path, page),
    };
    definition.handle = handle;
    let base = crate::entities::underlay::definition_display_name(&definition);
    if let Some(dictionary) = ensure_definitions_dictionary(document, kind) {
        definition.owner_handle = dictionary;
        if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&dictionary) {
            let mut key = base.clone();
            let mut suffix = 1;
            while entries.get(&key).is_some() {
                suffix += 1;
                key = format!("{base}({suffix})");
            }
            entries.add_entry(key, handle);
        }
    }
    document
        .objects
        .insert(handle, ObjectType::UnderlayDefinition(definition));
    handle
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["PDFATTACH", "-PDFATTACH", "DWFATTACH", "-DWFATTACH", "DGNATTACH", "-DGNATTACH"]
});
