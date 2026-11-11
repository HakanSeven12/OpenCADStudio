// GROUPEDIT: pick a group (or name it), then add objects to it, remove
// members from it or rename it. The edit itself runs through the typed
// `GROUP ADD | REMOVE | RENAME` options, so it is undone the same way.

use document::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::model::wire_model::WireModel;

enum Step {
    /// Pick a member of the group, or type Name.
    Pick,
    /// Type the group's name.
    Name,
    /// Add objects / Remove objects / REName.
    Option { group: String },
    /// Select the objects to add.
    Add { group: String },
    /// Pick members one by one to take out; Enter ends.
    Remove { group: String, picked: Vec<Handle> },
    /// Type the new name.
    Rename { group: String },
}

pub struct GroupEditCommand {
    /// Every group of the drawing: its name and members.
    groups: Vec<(String, Vec<Handle>)>,
    step: Step,
}

impl GroupEditCommand {
    pub fn new(groups: Vec<(String, Vec<Handle>)>) -> Self {
        Self {
            groups,
            step: Step::Pick,
        }
    }

    /// Start at the options for the group of a preselected object.
    pub fn for_selection(groups: Vec<(String, Vec<Handle>)>, selected: &[Handle]) -> Self {
        let mut command = Self::new(groups);
        if let Some(group) = selected.iter().find_map(|&h| command.group_of(h)) {
            command.step = Step::Option { group };
        }
        command
    }

    fn group_of(&self, handle: Handle) -> Option<String> {
        self.groups
            .iter()
            .find(|(_, members)| members.contains(&handle))
            .map(|(name, _)| name.clone())
    }

    fn named(&self, name: &str) -> Option<String> {
        self.groups
            .iter()
            .find(|(group, _)| group.eq_ignore_ascii_case(name))
            .map(|(group, _)| group.clone())
    }

    fn members(&self, group: &str) -> &[Handle] {
        self.groups
            .iter()
            .find(|(name, _)| name == group)
            .map_or(&[], |(_, members)| members)
    }
}

fn handle_list(handles: &[Handle]) -> String {
    handles
        .iter()
        .map(|h| format!("{:X}", h.value()))
        .collect::<Vec<_>>()
        .join(" ")
}

impl CadCommand for GroupEditCommand {
    fn name(&self) -> &'static str {
        "GROUPEDIT"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Pick => crate::t!("GROUPEDIT  Select group or [Name]:").into_owned(),
            Step::Name => crate::t!("GROUPEDIT  Enter a group name:").into_owned(),
            Step::Option { group } => crate::t!(
                "GROUPEDIT  Group \"%{group}\": Enter an option [Add objects/Remove objects/REName]:",
                group = group.as_str()
            )
            .into_owned(),
            Step::Add { .. } => crate::t!("GROUPEDIT  Select objects to add:").into_owned(),
            Step::Remove { .. } => {
                crate::t!("GROUPEDIT  Select objects to remove <done>:").into_owned()
            }
            Step::Rename { group } => crate::t!(
                "GROUPEDIT  Enter a new name for group \"%{group}\":",
                group = group.as_str()
            )
            .into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Pick => vec![CmdOption::new("Name", "N")],
            Step::Option { .. } => vec![
                CmdOption::new("Add objects", "A"),
                CmdOption::new("Remove objects", "R"),
                CmdOption::new("REName", "REN"),
            ],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Name | Step::Rename { .. } => InputKind::FreeText,
            _ => InputKind::default(),
        }
    }

    fn wants_text_input(&self) -> bool {
        !matches!(self.step, Step::Add { .. })
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let upper = text.to_ascii_uppercase();
        match &self.step {
            Step::Pick if upper == "N" || upper == "NAME" => {
                self.step = Step::Name;
                Some(CmdResult::NeedPoint)
            }
            Step::Pick | Step::Name => Some(match self.named(text) {
                Some(group) => {
                    self.step = Step::Option { group };
                    CmdResult::NeedPoint
                }
                None => CmdResult::ReportError(
                    crate::tf!("GROUPEDIT: no group named '{text}'.").into_owned(),
                ),
            }),
            Step::Option { group } => {
                let group = group.clone();
                self.step = match upper.as_str() {
                    "A" | "ADD" => Step::Add { group },
                    "R" | "REMOVE" => Step::Remove {
                        group,
                        picked: Vec::new(),
                    },
                    "REN" | "RENAME" => Step::Rename { group },
                    _ => return Some(CmdResult::NeedPoint),
                };
                Some(CmdResult::NeedPoint)
            }
            Step::Rename { group } => {
                if text.contains(char::is_whitespace) {
                    return Some(CmdResult::ReportError(
                        crate::t!("GROUPEDIT: a group name has no spaces.").into_owned(),
                    ));
                }
                Some(CmdResult::Dispatch(format!("GROUP RENAME {group} {text}")))
            }
            Step::Add { .. } | Step::Remove { .. } => None,
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::Pick | Step::Remove { .. })
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match &self.step {
            Step::Pick => match self.group_of(handle) {
                Some(group) => {
                    self.step = Step::Option { group };
                    CmdResult::NeedPoint
                }
                None => CmdResult::ReportError(
                    crate::t!("GROUPEDIT: that object is not in a group.").into_owned(),
                ),
            },
            // One member at a time: a pick here is never widened to the
            // whole group, as a selection set would be.
            Step::Remove { group, picked } => {
                let take = self.members(group).contains(&handle) && !picked.contains(&handle);
                if let (true, Step::Remove { picked, .. }) = (take, &mut self.step) {
                    picked.push(handle);
                }
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.step, Step::Add { .. })
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        let Step::Add { group } = &self.step else {
            return CmdResult::NeedPoint;
        };
        if handles.is_empty() {
            return CmdResult::Cancel;
        }
        CmdResult::Dispatch(format!("GROUP ADD {group} {}", handle_list(&handles)))
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        match &self.step {
            Step::Remove { group, picked } if !picked.is_empty() => {
                CmdResult::Dispatch(format!("GROUP REMOVE {group} {}", handle_list(picked)))
            }
            _ => CmdResult::Cancel,
        }
    }

    fn on_hover_entity(&mut self, _handle: Handle, _pt: DVec3) -> Vec<WireModel> {
        vec![]
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["GROUPEDIT"]
});

#[cfg(test)]
mod tests {
    use super::*;

    fn groups() -> Vec<(String, Vec<Handle>)> {
        vec![
            ("A".into(), vec![Handle::new(0x10), Handle::new(0x11)]),
            ("B".into(), vec![Handle::new(0x20)]),
        ]
    }

    fn dispatched(result: CmdResult) -> String {
        match result {
            CmdResult::Dispatch(line) => line,
            _ => panic!("expected a dispatch"),
        }
    }

    #[test]
    fn pick_a_member_then_remove_members_one_by_one() {
        let mut command = GroupEditCommand::new(groups());
        command.on_entity_pick(Handle::new(0x11), DVec3::ZERO);
        command.on_text_input("R");
        command.on_entity_pick(Handle::new(0x10), DVec3::ZERO);
        // Not a member: left alone.
        command.on_entity_pick(Handle::new(0x20), DVec3::ZERO);
        assert_eq!(dispatched(command.on_enter()), "GROUP REMOVE A 10");
    }

    #[test]
    fn name_a_group_then_add_or_rename() {
        let mut command = GroupEditCommand::new(groups());
        command.on_text_input("N");
        command.on_text_input("b");
        command.on_text_input("A");
        assert!(command.is_selection_gathering());
        let line = dispatched(command.on_selection_complete(vec![Handle::new(0x2A)]));
        assert_eq!(line, "GROUP ADD B 2A");

        let mut command = GroupEditCommand::for_selection(groups(), &[Handle::new(0x20)]);
        command.on_text_input("REN");
        let line = dispatched(command.on_text_input("C").expect("consumed"));
        assert_eq!(line, "GROUP RENAME B C");
    }
}
