// BVSTATE — the visibility states of the block open in the Block Editor:
// make a new one, make one current, rename or delete one. The first use in a
// block makes it dynamic with a visibility parameter.

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use codec::Handle;
use glam::DVec3;

/// Separates the fields of the `_BVSTATE` step the command hands over.
pub const SEP: char = '\u{1f}';

enum Step {
    Option,
    NewName,
    SetName,
    RenameFrom,
    RenameTo(String),
    DeleteName,
}

pub struct BvStateCommand {
    names: Vec<String>,
    current: usize,
    step: Step,
}

impl BvStateCommand {
    pub fn new(names: Vec<String>, current: usize) -> Self {
        Self {
            names,
            current,
            step: Step::Option,
        }
    }

    fn current_name(&self) -> String {
        self.names.get(self.current).cloned().unwrap_or_default()
    }

    fn known(&self, name: &str) -> Option<String> {
        self.names
            .iter()
            .find(|candidate| candidate.eq_ignore_ascii_case(name))
            .cloned()
    }

    fn done(parts: &[&str]) -> CmdResult {
        let mut command = String::from("_BVSTATE");
        for part in parts {
            command.push(SEP);
            command.push_str(part);
        }
        CmdResult::Dispatch(command)
    }

    fn unknown(name: &str) -> CmdResult {
        CmdResult::ReportError(crate::tf!("Visibility state \"{}\" not found.", name).into_owned())
    }
}

impl CadCommand for BvStateCommand {
    fn name(&self) -> &'static str {
        "BVSTATE"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Option => {
                crate::t!("BVSTATE  Enter an option [New/Set/Rename/Delete] <Set>:").into_owned()
            }
            Step::NewName => crate::t!("BVSTATE  Enter name of new visibility state:").into_owned(),
            Step::SetName => crate::tf!(
                "BVSTATE  Enter visibility state to make current <{}>:",
                self.current_name()
            )
            .into_owned(),
            Step::RenameFrom => crate::tf!(
                "BVSTATE  Enter visibility state to rename <{}>:",
                self.current_name()
            )
            .into_owned(),
            Step::RenameTo(_) => crate::t!("BVSTATE  Enter new name:").into_owned(),
            Step::DeleteName => {
                crate::t!("BVSTATE  Enter visibility state to delete:").into_owned()
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Option => vec![
                CmdOption::new(crate::t!("New").as_ref(), "N"),
                CmdOption::new(crate::t!("Set").as_ref(), "S"),
                CmdOption::new(crate::t!("Rename").as_ref(), "R"),
                CmdOption::new(crate::t!("Delete").as_ref(), "D"),
            ],
            _ => Vec::new(),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Option => InputKind::SingleToken,
            _ => InputKind::FreeText,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        match &self.step {
            Step::Option => {
                self.step = match text.trim_start_matches('_').to_ascii_uppercase().as_str() {
                    "N" | "NEW" => Step::NewName,
                    "" | "S" | "SET" => Step::SetName,
                    "R" | "RENAME" => Step::RenameFrom,
                    "D" | "DELETE" => Step::DeleteName,
                    _ => {
                        return Some(CmdResult::ReportError(
                            crate::t!("Invalid option keyword.").into_owned(),
                        ))
                    }
                };
                Some(CmdResult::NeedPoint)
            }
            Step::NewName if text.is_empty() => Some(CmdResult::NeedPoint),
            Step::NewName => match self.known(text) {
                Some(_) => Some(CmdResult::ReportError(
                    crate::tf!("Visibility state \"{}\" already exists.", text).into_owned(),
                )),
                None => Some(Self::done(&["NEW", text])),
            },
            Step::SetName => {
                let name = if text.is_empty() {
                    self.current_name()
                } else {
                    text.to_string()
                };
                Some(match self.known(&name) {
                    Some(name) => Self::done(&["SET", &name]),
                    None => Self::unknown(&name),
                })
            }
            Step::RenameFrom => {
                let name = if text.is_empty() {
                    self.current_name()
                } else {
                    text.to_string()
                };
                match self.known(&name) {
                    Some(name) => {
                        self.step = Step::RenameTo(name);
                        Some(CmdResult::NeedPoint)
                    }
                    None => Some(Self::unknown(&name)),
                }
            }
            Step::RenameTo(_) if text.is_empty() => Some(CmdResult::NeedPoint),
            Step::RenameTo(from) => {
                let taken = self
                    .known(text)
                    .is_some_and(|existing| !existing.eq_ignore_ascii_case(from));
                Some(if taken {
                    CmdResult::ReportError(
                        crate::tf!("Visibility state \"{}\" already exists.", text).into_owned(),
                    )
                } else {
                    Self::done(&["RENAME", from, text])
                })
            }
            Step::DeleteName if text.is_empty() => Some(CmdResult::NeedPoint),
            Step::DeleteName => Some(match self.known(text) {
                Some(_) if self.names.len() <= 1 => CmdResult::ReportError(
                    crate::t!("The only visibility state cannot be deleted.").into_owned(),
                ),
                Some(name) => Self::done(&["DELETE", &name]),
                None => Self::unknown(text),
            }),
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_entity_pick(&mut self, _handle: Handle, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").unwrap_or(CmdResult::Cancel)
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["BVSTATE", "BVHIDE", "BVSHOW"]
});
