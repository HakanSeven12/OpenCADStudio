use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult};

/// `-PARAMETERS`: the reference's command-line parameter manager. One run
/// performs one operation; the host answers the `LIST`, `NEW`, `EDIT`,
/// `SET`, `RENAME` and `DELETE` dispatches.
pub struct ParametersCliCommand {
    step: Step,
}

#[derive(Clone)]
enum Step {
    /// `Enter a parameter option [New/Edit/Rename/Delete/?]:`
    Option,
    /// `Enter name for new user parameter:`
    NewName,
    /// `Enter expression:`
    NewExpression { name: String },
    /// `Enter parameter name:`
    EditName,
    /// `Enter expression:` after the host printed the old expression.
    EditExpression { name: String },
    /// `Enter old parameter name:`
    RenameOld,
    /// `Enter new parameter name:`
    RenameNew { old: String },
    /// `Enter parameter name to delete:`
    DeleteName,
}

impl ParametersCliCommand {
    pub fn new() -> Self {
        Self { step: Step::Option }
    }

    /// The expression prompt the host opens after printing the old expression.
    pub fn edit_expression(name: String) -> Self {
        Self {
            step: Step::EditExpression { name },
        }
    }
}

impl Default for ParametersCliCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl CadCommand for ParametersCliCommand {
    fn name(&self) -> &'static str {
        "-PARAMETERS"
    }

    fn prompt(&self) -> String {
        let text = match &self.step {
            Step::Option => "Enter a parameter option [New/Edit/Rename/Delete/?]:",
            Step::NewName => "Enter name for new user parameter:",
            Step::NewExpression { .. } | Step::EditExpression { .. } => "Enter expression:",
            Step::EditName => "Enter parameter name:",
            Step::RenameOld => "Enter old parameter name:",
            Step::RenameNew { .. } => "Enter new parameter name:",
            Step::DeleteName => "Enter parameter name to delete:",
        };
        format!("-PARAMETERS  {text}")
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Option => vec![
                CmdOption::new("New", "N"),
                CmdOption::new("Edit", "E"),
                CmdOption::new("Rename", "R"),
                CmdOption::new("Delete", "D"),
                CmdOption::new("?", "?"),
            ],
            _ => Vec::new(),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        match self.step.clone() {
            Step::Option => {
                let keyword = text.trim_start_matches('_').to_ascii_uppercase();
                let next = match keyword.as_str() {
                    "N" | "NEW" => Step::NewName,
                    "E" | "EDIT" => Step::EditName,
                    "R" | "RENAME" => Step::RenameOld,
                    "D" | "DELETE" => Step::DeleteName,
                    "?" => return Some(CmdResult::Dispatch("-PARAMETERS LIST".to_string())),
                    _ => return None,
                };
                self.step = next;
                Some(CmdResult::NeedPoint)
            }
            Step::NewName => {
                self.step = Step::NewExpression {
                    name: text.to_string(),
                };
                Some(CmdResult::NeedPoint)
            }
            Step::NewExpression { name } => {
                Some(CmdResult::Dispatch(format!("-PARAMETERS NEW {name} {text}")))
            }
            Step::EditName => Some(CmdResult::Dispatch(format!("-PARAMETERS EDIT {text}"))),
            Step::EditExpression { name } => {
                Some(CmdResult::Dispatch(format!("-PARAMETERS SET {name} {text}")))
            }
            Step::RenameOld => {
                self.step = Step::RenameNew {
                    old: text.to_string(),
                };
                Some(CmdResult::NeedPoint)
            }
            Step::RenameNew { old } => {
                Some(CmdResult::Dispatch(format!("-PARAMETERS RENAME {old} {text}")))
            }
            Step::DeleteName => Some(CmdResult::Dispatch(format!("-PARAMETERS DELETE {text}"))),
        }
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
