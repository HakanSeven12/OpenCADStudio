// DVIEW command — turn the view about its line of sight (TWist).

use crate::t;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult};

pub struct DviewCommand {
    /// The view's twist when the command started or last changed, radians.
    current: f64,
    /// Earlier twists, for Undo.
    history: Vec<f64>,
    twist_prompt: bool,
}

impl DviewCommand {
    /// `current` is the view's present twist, counter-clockwise, in radians.
    pub fn new(current: f64) -> Self {
        Self {
            current,
            history: Vec::new(),
            twist_prompt: false,
        }
    }
}

impl CadCommand for DviewCommand {
    fn name(&self) -> &'static str {
        "DVIEW"
    }

    fn prompt(&self) -> String {
        if self.twist_prompt {
            let current = crate::entities::common::format_angle(self.current);
            crate::tf!("DVIEW  Specify view twist angle <{current}>:").into_owned()
        } else {
            t!("DVIEW  Enter option [TWist/Undo]:").into_owned()
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.twist_prompt {
            return Vec::new();
        }
        vec![CmdOption::new("TWist", "TW"), CmdOption::new("Undo", "U")]
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        if self.twist_prompt {
            let twist = crate::entities::common::parse_angle(&text.replace(',', "."))?;
            self.twist_prompt = false;
            self.history.push(self.current);
            self.current = twist;
            return Some(CmdResult::TwistView(twist));
        }
        match text.trim_start_matches('_').to_ascii_uppercase().as_str() {
            "TW" | "TWIST" => {
                self.twist_prompt = true;
                Some(CmdResult::NeedPoint)
            }
            "U" | "UNDO" => {
                let previous = self.history.pop()?;
                self.current = previous;
                Some(CmdResult::TwistView(previous))
            }
            _ => None,
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.twist_prompt {
            // Enter keeps the current twist and returns to the options.
            self.twist_prompt = false;
            return CmdResult::NeedPoint;
        }
        CmdResult::Cancel
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["DVIEW"] });
