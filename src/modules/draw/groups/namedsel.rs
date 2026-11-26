// NAMEDSEL: save the selection under a name, recall it, rename or delete a
// set, or list the sets. Each choice runs the typed `NAMEDSEL SAVE | RECALL |
// RENAME | DELETE | ?` form, so a change is undone the same way.

use document::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, CommandSuggestion, InputKind};
use crate::scene::model::wire_model::WireModel;

enum Step {
    /// Save / Recall / Rename / Delete / ?
    Option,
    /// Select the objects to save (Save with nothing selected).
    Select,
    /// Type the name to save the selection under.
    SaveName,
    /// Type the name of the set to recall.
    RecallName,
    /// Type the name of the set to rename.
    RenameOld,
    /// Type the new name.
    RenameNew { old: String },
    /// Type the name of the set to delete.
    DeleteName,
}

pub struct NamedSelCommand {
    /// The objects Save takes: the selection when the command started, or
    /// the objects selected at its own prompt.
    selection: Vec<Handle>,
    /// The drawing's set names, offered at the name steps.
    names: Vec<String>,
    step: Step,
}

impl NamedSelCommand {
    pub fn new(selection: Vec<Handle>, names: Vec<String>) -> Self {
        Self {
            selection,
            names,
            step: Step::Option,
        }
    }

    fn save(&self, name: &str) -> String {
        let mut text = format!("NAMEDSEL SAVE {name}");
        for handle in &self.selection {
            text.push_str(&format!(" {:X}", handle.value()));
        }
        text
    }
}

impl CadCommand for NamedSelCommand {
    fn name(&self) -> &'static str {
        "NAMEDSEL"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Option => {
                crate::t!("NAMEDSEL  Enter an option [Save/Recall/Rename/Delete/?]:").into_owned()
            }
            Step::Select => crate::t!("NAMEDSEL  Select objects:").into_owned(),
            Step::SaveName => {
                crate::t!("NAMEDSEL  Enter a name for the selection set:").into_owned()
            }
            Step::RecallName | Step::RenameOld | Step::DeleteName => {
                crate::t!("NAMEDSEL  Enter the selection set name:").into_owned()
            }
            Step::RenameNew { old } => crate::t!(
                "NAMEDSEL  Enter a new name for \"%{old}\":",
                old = old.as_str()
            )
            .into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Option => vec![
                CmdOption::new(crate::t!("Save").as_ref(), "S"),
                CmdOption::new(crate::t!("Recall").as_ref(), "R"),
                CmdOption::new(crate::t!("Rename").as_ref(), "REN"),
                CmdOption::new(crate::t!("Delete").as_ref(), "D"),
                CmdOption::new("?", "?"),
            ],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Option | Step::Select => InputKind::default(),
            _ => InputKind::FreeText,
        }
    }

    fn wants_text_input(&self) -> bool {
        !matches!(self.step, Step::Select)
    }

    fn text_suggestions(&self, input: &str) -> Vec<CommandSuggestion> {
        if !matches!(
            self.step,
            Step::RecallName | Step::RenameOld | Step::DeleteName
        ) {
            return Vec::new();
        }
        let input = input.trim().to_lowercase();
        self.names
            .iter()
            .filter(|name| name.to_lowercase().starts_with(&input))
            .map(|name| CommandSuggestion::plain(name.clone()))
            .collect()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        if let Step::Option = self.step {
            self.step = match text.to_ascii_uppercase().as_str() {
                "S" | "SAVE" if self.selection.is_empty() => Step::Select,
                "S" | "SAVE" => Step::SaveName,
                "R" | "RECALL" => Step::RecallName,
                "REN" | "RENAME" => Step::RenameOld,
                "D" | "DELETE" => Step::DeleteName,
                "?" | "LIST" => return Some(CmdResult::Dispatch("NAMEDSEL ?".to_string())),
                _ => return Some(CmdResult::NeedPoint),
            };
            return Some(CmdResult::NeedPoint);
        }
        if text.contains(char::is_whitespace) {
            return Some(CmdResult::ReportError(
                crate::t!("NAMEDSEL: a selection set name has no spaces.").into_owned(),
            ));
        }
        Some(match &self.step {
            Step::SaveName => CmdResult::Dispatch(self.save(text)),
            Step::RecallName => CmdResult::Dispatch(format!("NAMEDSEL RECALL {text}")),
            Step::RenameOld => {
                self.step = Step::RenameNew {
                    old: text.to_string(),
                };
                CmdResult::NeedPoint
            }
            Step::RenameNew { old } => CmdResult::Dispatch(format!("NAMEDSEL RENAME {old} {text}")),
            Step::DeleteName => CmdResult::Dispatch(format!("NAMEDSEL DELETE {text}")),
            Step::Option | Step::Select => CmdResult::NeedPoint,
        })
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.step, Step::Select)
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if !matches!(self.step, Step::Select) {
            return CmdResult::NeedPoint;
        }
        if handles.is_empty() {
            return CmdResult::Cancel;
        }
        self.selection = handles;
        self.step = Step::SaveName;
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_hover_entity(&mut self, _handle: Handle, _pt: DVec3) -> Vec<WireModel> {
        vec![]
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["NAMEDSEL"]
});
