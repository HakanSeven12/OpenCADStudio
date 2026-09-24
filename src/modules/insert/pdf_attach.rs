// PDFATTACH / -PDFATTACH — attach a PDF page as an underlay.
//
//   Path to PDF file to attach:                (-PDFATTACH only)
//   Enter page number or [?] <1>:
//   Specify insertion point:
//   Base image size: Width: 7.8740, Height: 3.9369, Meters
//   Specify scale factor or [Unit] <1>:
//   Specify rotation <0>:
//
// The definition (one per file and page, named "<file> - <page>" in the
// ACAD_PDFDEFINITIONS dictionary) is created with the underlay when it is
// placed, so a cancelled attach leaves nothing and undo removes both.

use std::sync::Mutex;

use acadrust::entities::{Underlay, UnderlayDisplayFlags};
use acadrust::objects::{Dictionary, ObjectType, UnderlayDefinition};
use acadrust::types::{Handle, Vector3};
use acadrust::{CadDocument, EntityType};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

pub const ICON: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/underlay_layers.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "PDFATTACH",
        label: "Attach PDF",
        icon: ICON,
        event: ModuleEvent::Command("PDFATTACH".to_string()),
    }
}

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
    Insertion,
    Scale,
    Unit,
    Rotation,
}

pub struct PdfAttachCommand {
    step: Step,
    /// The file read for the page (absolute or registered source).
    path: String,
    /// The path the definition stores (full, relative or file name only).
    stored_path: Option<String>,
    /// Pages after the first, attached beside it (dialog selection).
    extra_pages: Vec<String>,
    /// Scale and rotation fixed in the dialog: their prompts are skipped.
    preset_scale: Option<f64>,
    preset_rotation: Option<f64>,
    page: String,
    page_count: usize,
    page_size: (f64, f64),
    insertion: DVec3,
    scale: f64,
    unit: usize,
    plane: WorkingPlane,
}

impl PdfAttachCommand {
    /// `-PDFATTACH`: starts by asking for the file.
    pub fn new(insunits: i16) -> Self {
        let unit = LAST_UNIT
            .lock()
            .ok()
            .and_then(|last| *last)
            .unwrap_or_else(|| unit_for_insunits(insunits));
        Self {
            step: Step::Path,
            path: String::new(),
            stored_path: None,
            extra_pages: Vec::new(),
            preset_scale: None,
            preset_rotation: None,
            page: "1".to_string(),
            page_count: 0,
            page_size: (0.0, 0.0),
            insertion: DVec3::ZERO,
            scale: 1.0,
            unit,
            plane: WorkingPlane::default(),
        }
    }

    /// PDFATTACH with a file already chosen: starts at the page prompt.
    pub fn with_file(path: &str, insunits: i16) -> Self {
        let mut command = Self::new(insunits);
        if let Some(count) = crate::scene::model::pdf_raster::page_count(path) {
            command.path = path.to_string();
            command.page_count = count;
            command.step = Step::Page;
        }
        command
    }

    /// The dialog's attach with the insertion point asked on screen: pages
    /// chosen, the path to store, and any scale / rotation already given.
    pub fn from_dialog(
        read_path: &str,
        stored_path: &str,
        pages: &[String],
        scale: Option<f64>,
        rotation_deg: Option<f64>,
        insunits: i16,
    ) -> Self {
        let mut command = Self::with_file(read_path, insunits);
        command.stored_path = Some(stored_path.to_string());
        if let Some((first, rest)) = pages.split_first() {
            command.page = first.clone();
            command.extra_pages = rest.to_vec();
        }
        command.page_size =
            crate::scene::model::pdf_raster::page_size_inches(read_path, &command.page)
                .unwrap_or((0.0, 0.0));
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
            self.page_size.0, self.page_size.1, UNITS[self.unit].1
        )
    }

    fn accept_path(&mut self, text: &str) -> CmdResult {
        let mut path = text.trim().trim_matches('"').to_string();
        if path.is_empty() {
            return CmdResult::NeedPoint;
        }
        if std::path::Path::new(&path).extension().is_none() {
            path.push_str(".pdf");
        }
        match crate::scene::model::pdf_raster::page_count(&path) {
            Some(count) if count > 0 => {
                self.path = path;
                self.page_count = count;
                self.step = Step::Page;
                CmdResult::NeedPoint
            }
            _ => CmdResult::ReportError(format!(
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
        let page = if text.is_empty() { "1" } else { text };
        match page.parse::<usize>() {
            Ok(n) if n >= 1 && n <= self.page_count => {
                self.page = n.to_string();
                self.page_size = crate::scene::model::pdf_raster::page_size_inches(&self.path, &self.page)
                    .unwrap_or((0.0, 0.0));
                self.step = Step::Insertion;
                CmdResult::NeedPoint
            }
            _ => CmdResult::ReportError(format!(
                "There is no {page} in {}.",
                crate::entities::underlay::display_path(&self.path)
            )),
        }
    }

    fn list_pages(&mut self, pattern: &str) -> CmdResult {
        let pattern = if pattern.trim().is_empty() { "*" } else { pattern.trim() };
        let lines: Vec<String> = (1..=self.page_count)
            .map(|n| n.to_string())
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
            0.0
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
        self.commit(degrees.to_radians())
    }

    fn commit(&mut self, rotation: f64) -> CmdResult {
        let pages: Vec<String> = std::iter::once(self.page.clone())
            .chain(self.extra_pages.iter().cloned())
            .collect();
        let placed = underlays_for_pages(
            &self.path,
            &pages,
            self.plane.to_local(self.insertion),
            self.scale,
            rotation,
        )
        .into_iter()
        .map(|(page, underlay)| (page, self.plane.place_entity(underlay)))
        .collect();
        CmdResult::AttachPdfPages {
            path: self.stored_path.clone().unwrap_or_else(|| self.path.clone()),
            pages: placed,
        }
    }

    fn frame_at(&self, pt: DVec3) -> Vec<[f32; 3]> {
        let (w, h) = (self.page_size.0 * self.scale, self.page_size.1 * self.scale);
        let x = self.plane.vector_to_world(DVec3::new(w, 0.0, 0.0));
        let y = self.plane.vector_to_world(DVec3::new(0.0, h, 0.0));
        let corners = [pt, pt + x, pt + x + y, pt + y, pt];
        corners
            .iter()
            .map(|p| [p.x as f32, p.y as f32, p.z as f32])
            .collect()
    }
}

impl CadCommand for PdfAttachCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "PDFATTACH"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Path => "Path to PDF file to attach:".to_string(),
            Step::Page => "Enter page number or [?] <1>:".to_string(),
            Step::ListPages => "Enter page(s) to list <*>:".to_string(),
            Step::Insertion => "Specify insertion point:".to_string(),
            Step::Scale => "Specify scale factor or [Unit] <1>:".to_string(),
            Step::Unit => format!(
                "Enter unit [MM/Centimeter/Meter/Kilometer/Inch/Foot/Yard/MILe/Unitless] <{}>:",
                UNITS[self.unit].0
            ),
            Step::Rotation => "Specify rotation <0>:".to_string(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Page => vec![CmdOption::new("?", "?")],
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
            Step::Insertion => {
                CmdResult::ReportError("Point or option keyword required.".to_string())
            }
            Step::Scale => self.accept_scale(""),
            Step::Unit => self.accept_unit(""),
            Step::Rotation => self.accept_rotation(""),
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if self.step != Step::Insertion || self.page_size.0 <= 0.0 {
            return None;
        }
        Some(WireModel::solid(
            "pdf_attach_frame".into(),
            self.frame_at(pt),
            WireModel::CYAN,
            false,
        ))
    }
}

/// Underlays for `pages` of a file: the first at `insertion`, each next one
/// beside the previous along the rotated X axis (page width times scale).
// ponytail: side-by-side placement of the pages after the first is not
// measured against the reference.
pub fn underlays_for_pages(
    read_path: &str,
    pages: &[String],
    insertion: DVec3,
    scale: f64,
    rotation: f64,
) -> Vec<(String, EntityType)> {
    let mut origin = insertion;
    let (c, s) = (rotation.cos(), rotation.sin());
    pages
        .iter()
        .map(|page| {
            let mut underlay = Underlay::pdf();
            underlay.insertion_point = Vector3::new(origin.x, origin.y, origin.z);
            underlay.set_scale(scale);
            underlay.rotation = rotation;
            // On, clipped by its boundary and colour-adjusted for the
            // background, as the reference creates an underlay.
            underlay.flags = UnderlayDisplayFlags::ON
                | UnderlayDisplayFlags::CLIPPING
                | UnderlayDisplayFlags::ADJUST_FOR_BACKGROUND;
            let width = crate::scene::model::pdf_raster::page_size_inches(read_path, page)
                .map_or(0.0, |(w, _)| w * scale);
            origin += DVec3::new(c * width, s * width, 0.0);
            (page.clone(), EntityType::Underlay(underlay))
        })
        .collect()
}

/// The `ACAD_PDFDEFINITIONS` dictionary under the named-objects root,
/// created when the drawing has none.
fn ensure_pdf_dictionary(document: &mut CadDocument) -> Option<Handle> {
    let root = document.header.named_objects_dict_handle;
    let Some(ObjectType::Dictionary(root_dictionary)) = document.objects.get(&root) else {
        return None;
    };
    if let Some(existing) = root_dictionary.get("ACAD_PDFDEFINITIONS") {
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
        root_dictionary.add_entry("ACAD_PDFDEFINITIONS", handle);
    }
    Some(handle)
}

fn same_path(a: &str, b: &str) -> bool {
    let norm = |p: &str| p.replace('\\', "/").to_lowercase();
    norm(a) == norm(b)
}

/// The PDF definition for a file and page: the existing one, or a new one
/// registered in ACAD_PDFDEFINITIONS as "<file> - <page>".
pub fn ensure_pdf_definition(document: &mut CadDocument, path: &str, page: &str) -> Handle {
    let existing = document.objects.iter().find_map(|(handle, object)| match object {
        ObjectType::UnderlayDefinition(def)
            if matches!(def.underlay_type, acadrust::entities::UnderlayType::Pdf)
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
    let mut definition = UnderlayDefinition::pdf(path, page);
    definition.handle = handle;
    let base = crate::entities::underlay::definition_display_name(&definition);
    if let Some(dictionary) = ensure_pdf_dictionary(document) {
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
    names: &["PDFATTACH", "-PDFATTACH"]
});
