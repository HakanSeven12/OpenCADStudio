use crate::t;
use glam::{DVec2, DVec3};

use crate::command::{CadCommand, CmdOption, CmdResult};

enum LimitsStep {
    FirstCorner,
    OppositeCorner(DVec2),
}

/// Interactive front-end for LIMITS. The command itself only gathers input;
/// the dispatched form mutates the active drawing/layout in one central place.
pub struct LimitsCommand {
    step: LimitsStep,
    current_min: DVec2,
    current_max: DVec2,
}

impl LimitsCommand {
    pub fn new(current_min: DVec2, current_max: DVec2) -> Self {
        Self {
            step: LimitsStep::FirstCorner,
            current_min,
            current_max,
        }
    }

    fn point_text(point: DVec2) -> String {
        format!("{:.17} {:.17}", point.x, point.y)
    }
}

impl CadCommand for LimitsCommand {
    fn name(&self) -> &'static str {
        "LIMITS"
    }

    fn prompt(&self) -> String {
        match self.step {
            LimitsStep::FirstCorner => {
                let v = format!("{:.4},{:.4}", self.current_min.x, self.current_min.y);
                t!("LIMITS  Specify first corner or [On / Off] <%{v}>:", v = v).into_owned()
            }
            LimitsStep::OppositeCorner(_) => {
                let v = format!("{:.4},{:.4}", self.current_max.x, self.current_max.y);
                t!("LIMITS  Specify opposite corner <%{v}>:", v = v).into_owned()
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            LimitsStep::FirstCorner => vec![
                CmdOption::new(t!("On").as_ref(), "ON"),
                CmdOption::new(t!("Off").as_ref(), "OFF"),
            ],
            LimitsStep::OppositeCorner(_) => Vec::new(),
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if !matches!(self.step, LimitsStep::FirstCorner) {
            return None;
        }
        match text.trim().to_ascii_uppercase().as_str() {
            "ON" => Some(CmdResult::Dispatch("LIMITS ON".to_string())),
            "OFF" => Some(CmdResult::Dispatch("LIMITS OFF".to_string())),
            _ => None,
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        let point = point.truncate();
        match self.step {
            LimitsStep::FirstCorner => {
                self.step = LimitsStep::OppositeCorner(point);
                CmdResult::NeedPoint
            }
            LimitsStep::OppositeCorner(first) => CmdResult::Dispatch(format!(
                "LIMITS SET {} {}",
                Self::point_text(first),
                Self::point_text(point)
            )),
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            LimitsStep::FirstCorner => {
                self.step = LimitsStep::OppositeCorner(self.current_min);
                CmdResult::NeedPoint
            }
            LimitsStep::OppositeCorner(first) => CmdResult::Dispatch(format!(
                "LIMITS SET {} {}",
                Self::point_text(first),
                Self::point_text(self.current_max)
            )),
        }
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["LIMITS"] });
