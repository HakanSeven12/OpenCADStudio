//! CREATEPOINTMANUAL: COGO points one at a time, each asked for its
//! location, description and elevation and written as soon as it is
//! complete; Enter at the location ends the command, Esc drops only the
//! point in progress. The whole run is one undo step.

use crate::command::{CadCommand, CmdResult, InputKind};
use glam::DVec3;

enum Step {
    Location,
    Description(DVec3),
    Elevation(DVec3, Option<String>),
}

pub struct CreatePointManualCommand {
    step: Step,
    /// The description the next point offers: the last one given, none
    /// (".") at first.
    description: Option<String>,
}

impl CreatePointManualCommand {
    pub fn new() -> Self {
        Self { step: Step::Location, description: None }
    }

    fn shown(d: &Option<String>) -> &str {
        d.as_deref().unwrap_or(".")
    }

    /// The description typed at its prompt: Enter keeps the offered one,
    /// "." gives none.
    fn description_of(&self, typed: &str) -> Option<String> {
        match typed.trim() {
            "" => self.description.clone(),
            "." => None,
            t => Some(t.to_string()),
        }
    }
}

impl Default for CreatePointManualCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl CadCommand for CreatePointManualCommand {
    fn name(&self) -> &'static str {
        "CREATEPOINTMANUAL"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Location => crate::t!("Please specify a location for the new point:").into_owned(),
            Step::Description(_) => crate::tf!("Enter a point description <{}>:", Self::shown(&self.description)).into_owned(),
            Step::Elevation(..) => crate::t!("Specify a point elevation <.>:").into_owned(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Location => InputKind::Point,
            // Raw text: the elevation takes a number only, not an expression
            // or a point (measured: both are "Invalid input").
            Step::Description(_) | Step::Elevation(..) => InputKind::FreeText,
        }
    }

    fn groups_undo(&self) -> bool {
        true
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            // The elevation is asked for: the point's own z is not used.
            Step::Location => {
                self.step = Step::Description(DVec3::new(pt.x, pt.y, 0.0));
                CmdResult::NeedPoint
            }
            Step::Elevation(..) => CmdResult::ReportError(crate::t!("Invalid input").into_owned()),
            Step::Description(_) => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match std::mem::replace(&mut self.step, Step::Location) {
            Step::Location => None,
            Step::Description(at) => {
                let d = self.description_of(text);
                self.description = d.clone();
                self.step = Step::Elevation(at, d);
                Some(CmdResult::NeedPoint)
            }
            Step::Elevation(at, d) => {
                let t = text.trim();
                let elevation = match t {
                    "" | "." => None,
                    _ => match t.parse::<f64>() {
                        Ok(z) if z.is_finite() => Some(z),
                        _ => {
                            self.step = Step::Elevation(at, d);
                            return Some(CmdResult::ReportError(crate::t!("Invalid input").into_owned()));
                        }
                    },
                };
                Some(CmdResult::AddCogoPoint { position: at, description: d, elevation })
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Location => CmdResult::Done,
            _ => self.on_text_input("").unwrap_or(CmdResult::NeedPoint),
        }
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["CREATEPOINTMANUAL"] });
