// FIELD — places the field chosen in the Field dialog as a multi-line text,
// and the object pick behind the dialog's "Select object" button.

use codec::entities::mtext::AttachmentPoint;
use codec::entities::MText;
use codec::types::{Handle, Vector3};
use codec::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, InputKind, WorkingPlane};

/// A field to attach to the entity a command commits: the field code and the
/// objects it refers to.
pub type TextField = (String, Vec<Handle>);

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Point,
    Height,
    Justify,
}

/// `Specify start point or [Height/Justify]:` → an MTEXT holding the field.
pub struct FieldPlaceCommand {
    step: Step,
    value: String,
    field: TextField,
    style: String,
    height: f64,
    attachment: AttachmentPoint,
    plane: WorkingPlane,
    name: &'static str,
}

impl FieldPlaceCommand {
    pub fn new(value: String, field: TextField, style: String, height: f64) -> Self {
        Self {
            step: Step::Point,
            value,
            field,
            style,
            height,
            attachment: AttachmentPoint::TopLeft,
            plane: WorkingPlane::default(),
            name: "FIELD",
        }
    }

    /// The command name shown with the prompts (COUNTFIELD places its field
    /// as MTEXT).
    pub fn named(mut self, name: &'static str) -> Self {
        self.name = name;
        self
    }
}

fn short(v: f64) -> String {
    let t = format!("{v:.4}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl CadCommand for FieldPlaceCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Point => "Specify start point or [Height/Justify]:".into(),
            Step::Height => format!("Specify height <{}>:", short(self.height)),
            Step::Justify => "Enter justification [TL/TC/TR/ML/MC/MR/BL/BC/BR] <TL>:".into(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Justify => InputKind::SingleToken,
            _ => InputKind::Point,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.step == Step::Point
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let word = text.trim().trim_start_matches('_').to_ascii_uppercase();
        match self.step {
            Step::Point => match word.as_str() {
                "H" | "HEIGHT" => self.step = Step::Height,
                "J" | "JUSTIFY" => self.step = Step::Justify,
                "" => return Some(CmdResult::ReportError("Point or option keyword required.".into())),
                _ => return Some(CmdResult::ReportError("Point or option keyword required.".into())),
            },
            Step::Height => {
                if word.is_empty() {
                    self.step = Step::Point;
                    return Some(CmdResult::NeedPoint);
                }
                match word.parse::<f64>() {
                    Ok(v) if v > 0.0 => {
                        self.height = v;
                        self.step = Step::Point;
                    }
                    Ok(_) => return Some(CmdResult::ReportError("Value must be positive and nonzero.".into())),
                    Err(_) => return Some(CmdResult::ReportError("Requires numeric distance or two points.".into())),
                }
            }
            Step::Justify => {
                self.attachment = match word.as_str() {
                    "" | "TL" => AttachmentPoint::TopLeft,
                    "TC" => AttachmentPoint::TopCenter,
                    "TR" => AttachmentPoint::TopRight,
                    "ML" => AttachmentPoint::MiddleLeft,
                    "MC" => AttachmentPoint::MiddleCenter,
                    "MR" => AttachmentPoint::MiddleRight,
                    "BL" => AttachmentPoint::BottomLeft,
                    "BC" => AttachmentPoint::BottomCenter,
                    "BR" => AttachmentPoint::BottomRight,
                    _ => return Some(CmdResult::ReportError("Invalid option keyword.".into())),
                };
                self.step = Step::Point;
            }
        }
        Some(CmdResult::NeedPoint)
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.step == Step::Height {
            return CmdResult::NeedPoint;
        }
        let p = self.plane.to_local(pt);
        let mut mtext = MText::new();
        mtext.value = self.value.clone();
        mtext.insertion_point = Vector3::new(p.x, p.y, p.z);
        mtext.height = self.height;
        mtext.style = self.style.clone();
        mtext.attachment_point = self.attachment;
        // No defined width: the field text never wraps.
        mtext.rectangle_width = 0.0;
        CmdResult::CommitAndExit(self.plane.place_entity(EntityType::MText(mtext)))
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Point => CmdResult::ReportError("Point or option keyword required.".into()),
            _ => self.on_text_input("").unwrap_or(CmdResult::NeedPoint),
        }
    }

    fn text_field(&self) -> Option<TextField> {
        Some(self.field.clone())
    }
}

/// The Field dialog's "Select object": one object is picked and handed back
/// (`_FIELD_OBJECT <handle>`); Esc returns without one.
pub struct FieldObjectPickCommand;

impl CadCommand for FieldObjectPickCommand {
    fn name(&self) -> &'static str {
        "FIELD"
    }

    fn prompt(&self) -> String {
        "Select object:".into()
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        CmdResult::Dispatch(format!("_FIELD_OBJECT {:X}", handle.value()))
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Dispatch("_FIELD_OBJECT".into())
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Dispatch("_FIELD_OBJECT".into())
    }
}

/// The Field dialog's Average / Sum / Count / Cell buttons: a cell range (or
/// one cell) is picked in a table and handed back as
/// `_FIELD_CELL <function> <table> <x> <y> <z> [<table> <x> <y> <z>]`.
pub struct FieldTablePickCommand {
    pub function: &'static str,
    first: Option<(Handle, DVec3)>,
}

impl FieldTablePickCommand {
    pub fn new(function: &'static str) -> Self {
        Self { function, first: None }
    }
}

impl CadCommand for FieldTablePickCommand {
    fn name(&self) -> &'static str {
        "FIELD"
    }

    fn prompt(&self) -> String {
        match (self.function, self.first) {
            ("Cell", _) => "Select table cell:".into(),
            (_, None) => "Select first corner of table cell range:".into(),
            _ => "Select second corner of table cell range:".into(),
        }
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        let at = |h: Handle, p: DVec3| format!("{:X} {} {} {}", h.value(), p.x, p.y, p.z);
        match (self.function, self.first) {
            ("Cell", _) => CmdResult::Dispatch(format!("_FIELD_CELL Cell {}", at(handle, pt))),
            (_, None) => {
                self.first = Some((handle, pt));
                CmdResult::NeedPoint
            }
            (function, Some((first, first_pt))) => CmdResult::Dispatch(format!(
                "_FIELD_CELL {function} {} {}",
                at(first, first_pt),
                at(handle, pt)
            )),
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Dispatch("_FIELD_CELL".into())
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Dispatch("_FIELD_CELL".into())
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["FIELD"] });
