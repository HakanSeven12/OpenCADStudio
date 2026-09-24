// PDFCLIP / CLIP — clip a PDF underlay to a boundary.
//
//   Select PDF to clip:                      (CLIP: Select Object to clip:)
//   Enter PDF clipping option [ON/OFF/Delete/New boundary] <New boundary>:
//   Delete old boundary? [Yes/No] <Yes>:     (when one exists)
//   Outside mode - Objects outside boundary will be hidden.
//   Specify clipping boundary or select invert option:
//   [Select polyline/Polygonal/Rectangular/Invert clip] <Rectangular>:
//
// The boundary is stored in underlay units; the clip-inside bit follows the
// Invert choice.

use acadrust::entities::{Underlay, UnderlayDisplayFlags, UnderlayType};
use acadrust::types::{Handle, Vector2};
use acadrust::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::model::wire_model::WireModel;

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
}

pub struct PdfClipCommand {
    /// CLIP asks for any object; PDFCLIP for a PDF.
    generic: bool,
    step: Step,
    handle: Handle,
    underlay: Option<Underlay>,
    picked: Option<EntityType>,
    inverted: bool,
    points: Vec<DVec3>,
}

const OUTSIDE: &str = "Outside mode - Objects outside boundary will be hidden.";
const INSIDE: &str = "Inside mode - Objects inside boundary will be hidden.";
const BOUNDARY: &str = "Specify clipping boundary or select invert option:";

impl PdfClipCommand {
    pub fn new(generic: bool) -> Self {
        Self {
            generic,
            step: Step::Select,
            handle: Handle::NULL,
            underlay: None,
            picked: None,
            inverted: false,
            points: Vec::new(),
        }
    }

    fn finish(&self, underlay: Underlay) -> CmdResult {
        CmdResult::UpdateEntityAndFinish {
            handle: self.handle,
            entity: EntityType::Underlay(underlay),
        }
    }

    fn enter_mode(&mut self) -> CmdResult {
        self.step = Step::Mode;
        let mode = if self.inverted { INSIDE } else { OUTSIDE };
        CmdResult::ReportMeasurement(format!("{mode}\n{BOUNDARY}"))
    }

    fn to_local(u: &Underlay, p: DVec3) -> Vector2 {
        let (c, s) = (u.rotation.cos(), u.rotation.sin());
        let dx = p.x - u.insertion_point.x;
        let dy = p.y - u.insertion_point.y;
        let sx = if u.x_scale.abs() > 1e-12 { u.x_scale } else { 1.0 };
        let sy = if u.y_scale.abs() > 1e-12 { u.y_scale } else { 1.0 };
        Vector2::new((dx * c + dy * s) / sx, (-dx * s + dy * c) / sy)
    }

    fn apply_boundary(&self, world: &[DVec3]) -> CmdResult {
        let Some(mut underlay) = self.underlay.clone() else {
            return CmdResult::Cancel;
        };
        underlay.clip_boundary_vertices = world.iter().map(|p| Self::to_local(&underlay, *p)).collect();
        underlay.flags |= UnderlayDisplayFlags::CLIPPING;
        underlay.clip_inverted = self.inverted;
        self.finish(underlay)
    }

    fn option(&mut self, text: &str) -> CmdResult {
        let Some(mut underlay) = self.underlay.clone() else {
            return CmdResult::Cancel;
        };
        match text.trim().to_ascii_uppercase().as_str() {
            "ON" => {
                underlay.flags |= UnderlayDisplayFlags::CLIPPING;
                self.finish(underlay)
            }
            "OFF" => {
                underlay.flags -= UnderlayDisplayFlags::CLIPPING;
                self.finish(underlay)
            }
            "D" | "DELETE" => {
                underlay.clip_boundary_vertices.clear();
                underlay.flags -= UnderlayDisplayFlags::CLIPPING;
                self.finish(underlay)
            }
            "" | "N" | "NEW" | "NEW BOUNDARY" => {
                if underlay.clip_boundary_vertices.is_empty() {
                    self.enter_mode()
                } else {
                    self.step = Step::DeleteOld;
                    CmdResult::NeedPoint
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

    fn rectangle(a: DVec3, b: DVec3) -> Vec<DVec3> {
        vec![
            a,
            DVec3::new(b.x, a.y, a.z),
            b,
            DVec3::new(a.x, b.y, a.z),
        ]
    }
}

impl CadCommand for PdfClipCommand {
    fn name(&self) -> &'static str {
        if self.generic { "CLIP" } else { "PDFCLIP" }
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Select if self.generic => "Select Object to clip:".to_string(),
            Step::Select => "Select PDF to clip:".to_string(),
            Step::Option => {
                "Enter PDF clipping option [ON/OFF/Delete/New boundary] <New boundary>:".to_string()
            }
            Step::DeleteOld => "Delete old boundary? [Yes/No] <Yes>:".to_string(),
            Step::Mode => {
                "[Select polyline/Polygonal/Rectangular/Invert clip] <Rectangular>:".to_string()
            }
            Step::RectFirst => "Specify first corner point:".to_string(),
            Step::RectSecond => "Specify opposite corner point:".to_string(),
            Step::PolyPoints => match self.points.len() {
                0 => "Specify first point:".to_string(),
                1 | 2 => "Specify next point or [Undo]:".to_string(),
                _ => "Specify next point or [Close/Undo]:".to_string(),
            },
            Step::Polyline => "Select polyline:".to_string(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Option => vec![
                CmdOption::new("ON", "ON"),
                CmdOption::new("OFF", "OFF"),
                CmdOption::new("Delete", "D"),
                CmdOption::new("New boundary", "N"),
            ],
            Step::DeleteOld => vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")],
            Step::Mode => vec![
                CmdOption::new("Select polyline", "S"),
                CmdOption::new("Polygonal", "P"),
                CmdOption::new("Rectangular", "R"),
                CmdOption::new("Invert clip", "I"),
            ],
            Step::PolyPoints if self.points.len() >= 3 => {
                vec![CmdOption::new("Close", "C"), CmdOption::new("Undo", "U")]
            }
            Step::PolyPoints if !self.points.is_empty() => vec![CmdOption::new("Undo", "U")],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Option | Step::DeleteOld | Step::Mode => InputKind::SingleToken,
            _ => InputKind::Point,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.step == Step::PolyPoints
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::Select | Step::Polyline)
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        let picked = self.picked.take();
        match self.step {
            Step::Select => match picked {
                Some(EntityType::Underlay(u)) if u.underlay_type == UnderlayType::Pdf => {
                    self.handle = handle;
                    self.underlay = Some(u);
                    self.step = Step::Option;
                    CmdResult::NeedPoint
                }
                _ => CmdResult::ReportError("Object selected was not a PDF underlay.".to_string()),
            },
            Step::Polyline => match picked {
                Some(EntityType::LwPolyline(pl)) if pl.vertices.len() >= 3 => {
                    let z = pl.elevation;
                    let world: Vec<DVec3> = pl
                        .vertices
                        .iter()
                        .map(|v| DVec3::new(v.location.x, v.location.y, z))
                        .collect();
                    self.apply_boundary(&world)
                }
                _ => CmdResult::ReportError("Invalid object selected.".to_string()),
            },
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::RectFirst => {
                self.points = vec![pt];
                self.step = Step::RectSecond;
                CmdResult::NeedPoint
            }
            Step::RectSecond => {
                let first = self.points[0];
                self.apply_boundary(&Self::rectangle(first, pt))
            }
            Step::PolyPoints => {
                self.points.push(pt);
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        Some(match self.step {
            Step::Option => self.option(text),
            Step::DeleteOld => match text.trim().to_ascii_uppercase().as_str() {
                "" | "Y" | "YES" => self.enter_mode(),
                "N" | "NO" => match self.underlay.clone() {
                    Some(underlay) => self.finish(underlay),
                    None => CmdResult::Cancel,
                },
                _ => CmdResult::ReportError("Invalid option keyword.".to_string()),
            },
            Step::Mode => self.mode(text),
            Step::PolyPoints => match text.trim().to_ascii_uppercase().as_str() {
                "U" | "UNDO" => {
                    self.points.pop();
                    CmdResult::NeedPoint
                }
                "C" | "CLOSE" if self.points.len() >= 3 => {
                    let points = self.points.clone();
                    self.apply_boundary(&points)
                }
                _ => return None,
            },
            _ => return None,
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Option => self.option(""),
            Step::DeleteOld => self.enter_mode(),
            Step::Mode => self.mode(""),
            Step::PolyPoints if self.points.len() >= 3 => {
                let points = self.points.clone();
                self.apply_boundary(&points)
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
        let mut points: Vec<[f32; 3]> = ring.iter().map(|p| [p.x as f32, p.y as f32, p.z as f32]).collect();
        points.push(points[0]);
        Some(WireModel::solid("pdf_clip_boundary".into(), points, WireModel::CYAN, false))
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["PDFCLIP", "CLIP"]
});
