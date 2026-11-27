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

#[cfg(test)]
mod tests {
    use super::*;

    fn dispatched(result: Option<CmdResult>) -> String {
        match result {
            Some(CmdResult::Dispatch(text)) => text,
            _ => panic!("expected a dispatch"),
        }
    }

    fn command(selection: &[u64]) -> NamedSelCommand {
        NamedSelCommand::new(
            selection.iter().copied().map(Handle::new).collect(),
            vec!["FRAME".to_string()],
        )
    }

    #[test]
    fn each_option_dispatches_its_typed_form() {
        let mut save = command(&[0x2A, 0x31]);
        assert!(matches!(
            save.on_text_input("S"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            dispatched(save.on_text_input("Frame")),
            "NAMEDSEL SAVE Frame 2A 31"
        );

        let mut recall = command(&[]);
        let _ = recall.on_text_input("r");
        assert_eq!(
            dispatched(recall.on_text_input("FRAME")),
            "NAMEDSEL RECALL FRAME"
        );

        let mut rename = command(&[]);
        let _ = rename.on_text_input("REN");
        let _ = rename.on_text_input("FRAME");
        assert!(rename.prompt().contains("\"FRAME\""), "{}", rename.prompt());
        assert_eq!(
            dispatched(rename.on_text_input("BOX")),
            "NAMEDSEL RENAME FRAME BOX"
        );

        let mut delete = command(&[]);
        let _ = delete.on_text_input("D");
        assert_eq!(
            dispatched(delete.on_text_input("FRAME")),
            "NAMEDSEL DELETE FRAME"
        );

        let mut list = command(&[]);
        assert_eq!(dispatched(list.on_text_input("?")), "NAMEDSEL ?");
    }

    #[test]
    fn only_the_option_step_offers_options() {
        let mut command = command(&[]);
        let keys: Vec<String> = command
            .options()
            .into_iter()
            .map(|option| option.keyword)
            .collect();
        assert_eq!(keys, ["S", "R", "REN", "D", "?"]);
        let _ = command.on_text_input("R");
        assert!(command.options().is_empty());
    }

    #[test]
    fn save_with_no_selection_gathers_objects_first() {
        let mut save = command(&[]);
        assert!(!save.is_selection_gathering());
        let _ = save.on_text_input("SAVE");
        assert!(save.is_selection_gathering());
        assert!(matches!(
            save.on_selection_complete(Vec::new()),
            CmdResult::Cancel
        ));

        let mut save = command(&[]);
        let _ = save.on_text_input("SAVE");
        let _ = save.on_selection_complete(vec![Handle::new(0x2A)]);
        assert!(!save.is_selection_gathering());
        assert_eq!(dispatched(save.on_text_input("A")), "NAMEDSEL SAVE A 2A");
    }

    #[test]
    fn a_name_with_a_space_is_refused() {
        let mut save = command(&[0x2A]);
        let _ = save.on_text_input("S");
        assert!(matches!(
            save.on_text_input("TWO WORDS"),
            Some(CmdResult::ReportError(_))
        ));
        assert_eq!(
            dispatched(save.on_text_input("ONE")),
            "NAMEDSEL SAVE ONE 2A"
        );
    }

    #[test]
    fn the_name_steps_suggest_the_saved_names() {
        let mut recall = command(&[]);
        assert!(
            recall.text_suggestions("F").is_empty(),
            "no names at the option step"
        );
        let _ = recall.on_text_input("R");
        let names: Vec<String> = recall
            .text_suggestions("f")
            .into_iter()
            .map(|suggestion| suggestion.value)
            .collect();
        assert_eq!(names, ["FRAME"]);
    }
}
