// VISIBILITYSTATES — the visibility states of a BricsCAD parametric block, in
// model space of the block's own drawing: name a parameter and a state (new
// ones are made), then add, remove, show or hide objects in it, or make it
// current. The prompts follow BricsCAD's.

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use document::Handle;
use glam::DVec3;

/// Separates the fields of the steps the command hands over.
pub const SEP: char = '\u{1f}';

enum Step {
    Parameter,
    State(String),
    Action(String, String),
}

pub struct VisibilityStatesCommand {
    /// Each parameter with its states.
    parameters: Vec<(String, Vec<String>)>,
    step: Step,
}

impl VisibilityStatesCommand {
    pub fn new(parameters: Vec<(String, Vec<String>)>) -> Self {
        Self {
            parameters,
            step: Step::Parameter,
        }
    }

    fn known<'a>(names: impl IntoIterator<Item = &'a String>, name: &str) -> Option<String> {
        names
            .into_iter()
            .find(|candidate| candidate.eq_ignore_ascii_case(name))
            .cloned()
    }

    fn states(&self, parameter: &str) -> Vec<String> {
        self.parameters
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(parameter))
            .map(|(_, states)| states.clone())
            .unwrap_or_default()
    }

    fn done(parts: &[&str]) -> CmdResult {
        let mut command = String::from("_VISIBILITYSTATES");
        for part in parts {
            command.push(SEP);
            command.push_str(part);
        }
        CmdResult::Dispatch(command)
    }
}

impl CadCommand for VisibilityStatesCommand {
    fn name(&self) -> &'static str {
        "VISIBILITYSTATES"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Parameter => {
                let names: Vec<String> = self.parameters.iter().map(|(n, _)| n.clone()).collect();
                if names.is_empty() {
                    crate::t!("VISIBILITYSTATES  Enter a new parameter:").into_owned()
                } else {
                    crate::tf!(
                        "VISIBILITYSTATES  Enter a new parameter or [{}]:",
                        names.join("/")
                    )
                    .into_owned()
                }
            }
            Step::State(parameter) => {
                let states = self.states(parameter);
                if states.is_empty() {
                    crate::tf!("VISIBILITYSTATES  Enter a new state for {}:", parameter)
                        .into_owned()
                } else {
                    crate::tf!(
                        "VISIBILITYSTATES  Enter a new state for {} or [{}]:",
                        parameter,
                        states.join("/")
                    )
                    .into_owned()
                }
            }
            Step::Action(..) => crate::t!(
                "VISIBILITYSTATES  Select action [Add objects/Remove objects/make objects Visible/make objects Invisible/Select state] <Add objects>:"
            )
            .into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match &self.step {
            Step::Parameter => self
                .parameters
                .iter()
                .map(|(name, _)| CmdOption::new(name, name))
                .collect(),
            Step::State(parameter) => self
                .states(parameter)
                .iter()
                .map(|state| CmdOption::new(state, state))
                .collect(),
            Step::Action(..) => vec![
                CmdOption::new(crate::t!("Add objects").as_ref(), "A"),
                CmdOption::new(crate::t!("Remove objects").as_ref(), "R"),
                CmdOption::new(crate::t!("make objects Visible").as_ref(), "V"),
                CmdOption::new(crate::t!("make objects Invisible").as_ref(), "I"),
                CmdOption::new(crate::t!("Select state").as_ref(), "S"),
            ],
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Action(..) => InputKind::SingleToken,
            _ => InputKind::FreeText,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        match &self.step {
            Step::Parameter if text.is_empty() => Some(CmdResult::Cancel),
            Step::Parameter => {
                let names: Vec<String> = self.parameters.iter().map(|(n, _)| n.clone()).collect();
                let name = Self::known(&names, text).unwrap_or_else(|| text.to_string());
                self.step = Step::State(name);
                Some(CmdResult::NeedPoint)
            }
            Step::State(_) if text.is_empty() => Some(CmdResult::NeedPoint),
            Step::State(parameter) => {
                let parameter = parameter.clone();
                let state =
                    Self::known(&self.states(&parameter), text).unwrap_or_else(|| text.to_string());
                self.step = Step::Action(parameter, state);
                Some(CmdResult::NeedPoint)
            }
            Step::Action(parameter, state) => {
                // A translated prompt hands back the whole source option.
                let keyword = text.trim_start_matches('_').to_ascii_uppercase();
                let action = match keyword.as_str() {
                    "" | "A" | "ADD" | "ADD OBJECTS" => "ADD",
                    "R" | "REMOVE" | "REMOVE OBJECTS" => "REMOVE",
                    "V" | "VISIBLE" | "MAKE OBJECTS VISIBLE" => "VISIBLE",
                    "I" | "INVISIBLE" | "MAKE OBJECTS INVISIBLE" => "INVISIBLE",
                    "S" | "SELECT" | "ST" | "STATE" | "SELECT STATE" => "SELECT",
                    _ => {
                        return Some(CmdResult::ReportError(
                            crate::t!("Invalid option keyword.").into_owned(),
                        ))
                    }
                };
                Some(Self::done(&[parameter, state, action]))
            }
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
    names: &[
        "VISIBILITYSTATES",
        "VISIBILITYSTATESPANELOPEN",
        "VISIBILITYSTATESPANELCLOSE"
    ]
});

#[cfg(test)]
mod tests {
    use super::*;

    fn command() -> VisibilityStatesCommand {
        VisibilityStatesCommand::new(vec![("LAMP".into(), vec!["On".into(), "Off".into()])])
    }

    fn dispatched(result: Option<CmdResult>) -> Vec<String> {
        match result {
            Some(CmdResult::Dispatch(command)) => command.split(SEP).map(str::to_string).collect(),
            _ => panic!("the command hands nothing over"),
        }
    }

    #[test]
    fn known_names_take_their_own_case_and_the_action_defaults_to_add() {
        let mut command = command();
        assert!(command.prompt().contains("[LAMP]"), "{}", command.prompt());
        command.on_text_input("lamp");
        assert!(
            command.prompt().contains("[On/Off]"),
            "{}",
            command.prompt()
        );
        command.on_text_input("off");
        assert_eq!(
            dispatched(command.on_text_input("")),
            ["_VISIBILITYSTATES", "LAMP", "Off", "ADD"]
        );
    }

    #[test]
    fn new_names_and_each_action_keyword() {
        for (keyword, action) in [
            ("_A", "ADD"),
            ("R", "REMOVE"),
            ("v", "VISIBLE"),
            ("_Invisible", "INVISIBLE"),
            ("S", "SELECT"),
            ("make objects Visible", "VISIBLE"),
            ("Select state", "SELECT"),
        ] {
            let mut command = command();
            command.on_text_input("Label");
            command.on_text_input("Shown");
            assert_eq!(
                dispatched(command.on_text_input(keyword)),
                ["_VISIBILITYSTATES", "Label", "Shown", action]
            );
        }
        let mut command = command();
        command.on_text_input("LAMP");
        command.on_text_input("On");
        assert!(matches!(
            command.on_text_input("X"),
            Some(CmdResult::ReportError(_))
        ));
    }
}
