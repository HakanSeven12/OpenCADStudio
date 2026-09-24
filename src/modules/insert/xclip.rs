// XCLIP — clip block references (and drawing references) to a boundary.
//
//   Select objects:                          (block references only;
//                                             "N was ineligible for clipping.")
//   Enter clipping option
//   [ON/OFF/Clipdepth/Delete/generate Polyline/New boundary] <New>:
//   Delete old boundary(s)? [Yes/No] <Yes>:  (New, when one exists)
//   Outside mode - Objects outside boundary will be hidden.
//   Specify clipping boundary or select invert option:
//   [Select polyline/Polygonal/Rectangular/Invert clip] <Rectangular>:
//   Specify first corner: / Specify opposite corner:
//   Specify first point: / Specify next point or [Undo]:
//   Specify front clip point or [Distance/Remove]:
//
// CLIP asks for one object: a block reference continues as XCLIP, a PDF
// underlay as PDFCLIP, anything else is "*Invalid selection*".

use acadrust::entities::UnderlayType;
use acadrust::types::Handle;
use acadrust::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::modules::insert::pdf_clip::PdfClipCommand;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/xclip.svg"));
pub fn tool() -> ToolDef {
    ToolDef {
        id: "CLIP",
        label: "Clip",
        icon: ICON,
        event: ModuleEvent::Command("CLIP".to_string()),
    }
}

/// What XCLIP does to the chosen block references.
#[derive(Clone, Debug, PartialEq)]
pub enum XclipAction {
    Enable(bool),
    Delete,
    Polyline,
    /// Boundary in WCS (two points = rectangle corners).
    New { boundary: Vec<[f64; 2]>, inverted: bool },
    /// Front and back clipping planes; `None` removes one.
    Depth { front: Option<f64>, back: Option<f64> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Select,
    Option,
    DeleteOld,
    Mode,
    RectFirst,
    RectSecond,
    PolyPoints,
    Polyline,
    Front,
    FrontDistance,
    Back,
    BackDistance,
}

const OPTION: &str = "Enter clipping option\n[ON/OFF/Clipdepth/Delete/generate Polyline/New boundary] <New>:";
const OUTSIDE: &str = "Outside mode - Objects outside boundary will be hidden.";
const INSIDE: &str = "Inside mode - Objects inside boundary will be hidden.";
const BOUNDARY: &str = "Specify clipping boundary or select invert option:";

pub struct XclipCommand {
    step: Step,
    inserts: Vec<Handle>,
    /// Some chosen reference already has a clip boundary.
    clipped: bool,
    inverted: bool,
    points: Vec<DVec3>,
    picked: Option<EntityType>,
    front: Option<f64>,
}

impl XclipCommand {
    pub fn new() -> Self {
        Self {
            step: Step::Select,
            inserts: Vec::new(),
            clipped: false,
            inverted: false,
            points: Vec::new(),
            picked: None,
            front: None,
        }
    }

    /// The references chosen before the command (or by CLIP), with whether
    /// any of them is clipped already: straight to the option prompt.
    pub fn for_inserts(inserts: Vec<Handle>, clipped: bool) -> Self {
        Self {
            step: Step::Option,
            inserts,
            clipped,
            ..Self::new()
        }
    }

    fn act(&self, action: XclipAction) -> CmdResult {
        CmdResult::XClip {
            inserts: self.inserts.clone(),
            action,
        }
    }

    fn enter_mode(&mut self) -> CmdResult {
        self.step = Step::Mode;
        let mode = if self.inverted { INSIDE } else { OUTSIDE };
        CmdResult::ReportMeasurement(format!("{mode}\n{BOUNDARY}"))
    }

    fn new_boundary(&self, world: Vec<DVec3>) -> CmdResult {
        self.act(XclipAction::New {
            boundary: world.iter().map(|p| [p.x, p.y]).collect(),
            inverted: self.inverted,
        })
    }

    fn option(&mut self, text: &str) -> CmdResult {
        let token = text.trim().to_ascii_uppercase();
        let needs_boundary = matches!(
            token.as_str(),
            "ON" | "OFF" | "D" | "DELETE" | "P" | "POLYLINE" | "GENERATE POLYLINE" | "C" | "CLIPDEPTH"
        );
        if needs_boundary && !self.clipped {
            return CmdResult::ReportError("No clip boundary found.".to_string());
        }
        match token.as_str() {
            "ON" => self.act(XclipAction::Enable(true)),
            "OFF" => self.act(XclipAction::Enable(false)),
            "D" | "DELETE" => self.act(XclipAction::Delete),
            "P" | "POLYLINE" | "GENERATE POLYLINE" => self.act(XclipAction::Polyline),
            "C" | "CLIPDEPTH" => {
                self.step = Step::Front;
                CmdResult::NeedPoint
            }
            "" | "N" | "NEW" | "NEW BOUNDARY" => {
                if self.clipped {
                    self.step = Step::DeleteOld;
                    CmdResult::NeedPoint
                } else {
                    self.enter_mode()
                }
            }
            _ => CmdResult::ReportError("Invalid option keyword.".to_string()),
        }
    }

    fn mode(&mut self, text: &str) -> CmdResult {
        match text.trim().to_ascii_uppercase().as_str() {
            "" | "R" | "RECTANGULAR" => {
                self.step = Step::RectFirst;
                CmdResult::NeedPoint
            }
            "P" | "POLYGONAL" => {
                self.points.clear();
                self.step = Step::PolyPoints;
                CmdResult::NeedPoint
            }
            "S" | "SELECT" | "SELECT POLYLINE" => {
                self.step = Step::Polyline;
                CmdResult::NeedPoint
            }
            "I" | "INVERT" | "INVERT CLIP" => {
                self.inverted = !self.inverted;
                self.enter_mode()
            }
            _ => CmdResult::ReportError("Invalid option keyword.".to_string()),
        }
    }

    fn depth(&mut self, text: &str) -> CmdResult {
        match (self.step, text.trim().to_ascii_uppercase().as_str()) {
            (_, "R" | "REMOVE") => self.act(XclipAction::Depth { front: None, back: None }),
            (Step::Front, "D" | "DISTANCE") => {
                self.step = Step::FrontDistance;
                CmdResult::NeedPoint
            }
            (Step::Back, "D" | "DISTANCE") => {
                self.step = Step::BackDistance;
                CmdResult::NeedPoint
            }
            (Step::FrontDistance | Step::BackDistance, value) => match value.parse::<f64>() {
                Ok(d) => self.take_depth(d),
                Err(_) => CmdResult::ReportError("Requires numeric distance.".to_string()),
            },
            _ => CmdResult::ReportError("Invalid option keyword.".to_string()),
        }
    }

    fn take_depth(&mut self, d: f64) -> CmdResult {
        match self.step {
            Step::Front | Step::FrontDistance => {
                self.front = Some(d);
                self.step = Step::Back;
                CmdResult::NeedPoint
            }
            _ => self.act(XclipAction::Depth { front: self.front, back: Some(d) }),
        }
    }

    fn rectangle(a: DVec3, b: DVec3) -> Vec<DVec3> {
        vec![a, DVec3::new(b.x, a.y, a.z), b, DVec3::new(a.x, b.y, a.z)]
    }
}

impl CadCommand for XclipCommand {
    fn name(&self) -> &'static str {
        "XCLIP"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Select => "Select objects:".to_string(),
            Step::Option => OPTION.to_string(),
            Step::DeleteOld => "Delete old boundary(s)? [Yes/No] <Yes>:".to_string(),
            Step::Mode => {
                "[Select polyline/Polygonal/Rectangular/Invert clip] <Rectangular>:".to_string()
            }
            Step::RectFirst => "Specify first corner:".to_string(),
            Step::RectSecond => "Specify opposite corner:".to_string(),
            Step::PolyPoints if self.points.is_empty() => "Specify first point:".to_string(),
            Step::PolyPoints => "Specify next point or [Undo]:".to_string(),
            Step::Polyline => "Select polyline:".to_string(),
            Step::Front => "Specify front clip point or [Distance/Remove]:".to_string(),
            Step::Back => "Specify back clip point or [Distance/Remove]:".to_string(),
            Step::FrontDistance | Step::BackDistance => {
                "Specify distance from boundary:".to_string()
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Option => vec![
                CmdOption::new("ON", "ON"),
                CmdOption::new("OFF", "OFF"),
                CmdOption::new("Clipdepth", "C"),
                CmdOption::new("Delete", "D"),
                CmdOption::new("generate Polyline", "P"),
                CmdOption::new("New boundary", "N"),
            ],
            Step::DeleteOld => vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")],
            Step::Mode => vec![
                CmdOption::new("Select polyline", "S"),
                CmdOption::new("Polygonal", "P"),
                CmdOption::new("Rectangular", "R"),
                CmdOption::new("Invert clip", "I"),
            ],
            Step::PolyPoints if !self.points.is_empty() => vec![CmdOption::new("Undo", "U")],
            Step::Front | Step::Back => {
                vec![CmdOption::new("Distance", "D"), CmdOption::new("Remove", "R")]
            }
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Option | Step::DeleteOld | Step::Mode => InputKind::SingleToken,
            _ => InputKind::Point,
        }
    }

    fn wants_text_input(&self) -> bool {
        self.step != Step::Select
    }

    fn is_selection_gathering(&self) -> bool {
        self.step == Step::Select
    }

    fn selection_keeps_block_references(&self) -> bool {
        self.step == Step::Select
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.inserts = handles;
        CmdResult::NeedPoint
    }

    fn inject_clipped(&mut self, clipped: &dyn Fn(Handle) -> bool) {
        self.clipped = self.inserts.iter().any(|h| clipped(*h));
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(
            self.step,
            Step::PolyPoints | Step::Front | Step::Back | Step::FrontDistance | Step::BackDistance
        )
    }

    fn needs_entity_pick(&self) -> bool {
        self.step == Step::Polyline
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, _handle: Handle, _pt: DVec3) -> CmdResult {
        match self.picked.take() {
            Some(EntityType::LwPolyline(pl)) if pl.is_closed && pl.vertices.len() >= 3 => {
                let z = pl.elevation;
                self.new_boundary(
                    pl.vertices
                        .iter()
                        .map(|v| DVec3::new(v.location.x, v.location.y, z))
                        .collect(),
                )
            }
            _ => CmdResult::ReportError("Invalid object selected.".to_string()),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::RectFirst => {
                self.points = vec![pt];
                self.step = Step::RectSecond;
                CmdResult::NeedPoint
            }
            Step::RectSecond => self.new_boundary(vec![self.points[0], pt]),
            Step::PolyPoints => {
                self.points.push(pt);
                CmdResult::NeedPoint
            }
            // A clip point sets the plane at its height above the boundary.
            Step::Front | Step::Back => self.take_depth(pt.z),
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        Some(match self.step {
            Step::Option => self.option(text),
            Step::DeleteOld => match text.trim().to_ascii_uppercase().as_str() {
                "" | "Y" | "YES" => self.enter_mode(),
                "N" | "NO" => CmdResult::Cancel,
                _ => CmdResult::ReportError("Invalid option keyword.".to_string()),
            },
            Step::Mode => self.mode(text),
            Step::PolyPoints => match text.trim().to_ascii_uppercase().as_str() {
                "U" | "UNDO" => {
                    self.points.pop();
                    CmdResult::NeedPoint
                }
                _ => return None,
            },
            Step::Front | Step::Back | Step::FrontDistance | Step::BackDistance => self.depth(text),
            _ => return None,
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Select if self.inserts.is_empty() => {
                CmdResult::CancelWithMessage("None found.".to_string())
            }
            Step::Select => {
                self.step = Step::Option;
                CmdResult::NeedPoint
            }
            Step::Option => self.option(""),
            Step::DeleteOld => self.enter_mode(),
            Step::Mode => self.mode(""),
            Step::PolyPoints if self.points.len() >= 3 => {
                let points = self.points.clone();
                self.new_boundary(points)
            }
            Step::PolyPoints => CmdResult::NeedPoint,
            _ => CmdResult::Cancel,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let ring: Vec<DVec3> = match self.step {
            Step::RectSecond => Self::rectangle(self.points[0], pt),
            Step::PolyPoints if !self.points.is_empty() => {
                let mut ring = self.points.clone();
                ring.push(pt);
                ring
            }
            _ => return None,
        };
        let mut points: Vec<[f64; 3]> = ring.iter().map(|p| p.to_array()).collect();
        points.push(points[0]);
        Some(WireModel::solid_f64("xclip_boundary".into(), points, WireModel::CYAN, false))
    }
}

/// CLIP: one object, then that object's clip command.
pub struct ClipCommand {
    inner: Option<Box<dyn CadCommand>>,
    picked: Option<EntityType>,
}

impl ClipCommand {
    pub fn new() -> Self {
        Self {
            inner: None,
            picked: None,
        }
    }
}

impl CadCommand for ClipCommand {
    fn name(&self) -> &'static str {
        self.inner.as_ref().map_or("CLIP", |c| c.name())
    }
    fn prompt(&self) -> String {
        self.inner
            .as_ref()
            .map_or_else(|| "Select Object to clip:".to_string(), |c| c.prompt())
    }
    fn options(&self) -> Vec<CmdOption> {
        self.inner.as_ref().map_or_else(Vec::new, |c| c.options())
    }
    fn input_kind(&self) -> InputKind {
        self.inner.as_ref().map_or(InputKind::Point, |c| c.input_kind())
    }
    fn point_step_accepts_keywords(&self) -> bool {
        self.inner.as_ref().is_some_and(|c| c.point_step_accepts_keywords())
    }
    fn needs_entity_pick(&self) -> bool {
        self.inner.as_ref().is_none_or(|c| c.needs_entity_pick())
    }
    fn inject_before_entity_pick(&self) -> bool {
        true
    }
    fn inject_picked_entity(&mut self, entity: EntityType) {
        match self.inner.as_mut() {
            Some(c) => c.inject_picked_entity(entity),
            None => self.picked = Some(entity),
        }
    }
    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        if let Some(c) = self.inner.as_mut() {
            return c.on_entity_pick(handle, pt);
        }
        match self.picked.take() {
            Some(EntityType::Insert(_)) => {
                self.inner = Some(Box::new(XclipCommand::for_inserts(vec![handle], false)));
                CmdResult::NeedPoint
            }
            Some(EntityType::Underlay(u)) if u.underlay_type == UnderlayType::Pdf => {
                self.inner = Some(Box::new(PdfClipCommand::for_underlay(handle, u)));
                CmdResult::NeedPoint
            }
            _ => CmdResult::ReportError("*Invalid selection*".to_string()),
        }
    }
    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.inner.as_mut().map_or(CmdResult::NeedPoint, |c| c.on_point(pt))
    }
    fn inject_clipped(&mut self, clipped: &dyn Fn(Handle) -> bool) {
        if let Some(c) = self.inner.as_mut() {
            c.inject_clipped(clipped);
        }
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        self.inner.as_mut().and_then(|c| c.on_text_input(text))
    }
    fn on_enter(&mut self) -> CmdResult {
        self.inner.as_mut().map_or(CmdResult::Cancel, |c| c.on_enter())
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        self.inner.as_mut().and_then(|c| c.on_mouse_move(pt))
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["XCLIP", "CLIP"]
});
