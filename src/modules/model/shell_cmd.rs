use acadrust::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, ShellFaceAction};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellStep {
    SolidEditOption,
    BodyOption,
    SelectSolid,
    RemoveFaces,
    AddFaces,
    Distance,
}

pub struct ShellCommand {
    entry_name: &'static str,
    step: ShellStep,
    target: Option<Handle>,
    actions: Vec<ShellFaceAction>,
    default_distance: f64,
}

impl ShellCommand {
    pub fn direct(target: Option<Handle>) -> Self {
        Self {
            entry_name: "SHELL",
            step: if target.is_some() {
                ShellStep::RemoveFaces
            } else {
                ShellStep::SelectSolid
            },
            target,
            actions: Vec::new(),
            default_distance: 1.0,
        }
    }

    pub fn solid_edit(target: Option<Handle>) -> Self {
        Self {
            entry_name: "SOLIDEDIT",
            step: ShellStep::SolidEditOption,
            target,
            actions: Vec::new(),
            default_distance: 1.0,
        }
    }

    fn finish(&mut self, distance: f64) -> CmdResult {
        let Some(handle) = self.target else {
            return CmdResult::Cancel;
        };
        CmdResult::SolidShell {
            handle,
            actions: std::mem::take(&mut self.actions),
            distance,
        }
    }

    fn undo_face_action(&mut self) -> CmdResult {
        self.actions.pop();
        CmdResult::NeedPoint
    }

    fn face_options(add: bool) -> Vec<CmdOption> {
        if add {
            vec![
                CmdOption::new("Undo", "UNDO"),
                CmdOption::new("Remove", "REMOVE"),
                CmdOption::new("ALL", "ALL"),
            ]
        } else {
            vec![
                CmdOption::new("Undo", "UNDO"),
                CmdOption::new("Add", "ADD"),
                CmdOption::new("ALL", "ALL"),
            ]
        }
    }
}

impl CadCommand for ShellCommand {
    fn name(&self) -> &'static str {
        self.entry_name
    }

    fn prompt(&self) -> String {
        match self.step {
            ShellStep::SolidEditOption => {
                crate::t!("Enter a solid editing option [Body/eXit]:").into_owned()
            }
            ShellStep::BodyOption => {
                crate::t!("Enter a body editing option [Shell/eXit]:").into_owned()
            }
            ShellStep::SelectSolid => crate::t!("Select a 3D solid:").into_owned(),
            ShellStep::RemoveFaces => {
                crate::t!("Select faces to remove [Undo/Add/ALL] <done>:").into_owned()
            }
            ShellStep::AddFaces => {
                crate::t!("Select faces to add [Undo/Remove/ALL] <done>:").into_owned()
            }
            ShellStep::Distance => crate::tf!(
                "Specify the shell offset distance <{:.3}>:",
                self.default_distance
            )
            .into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            ShellStep::SolidEditOption => vec![
                CmdOption::new("Body", "BODY"),
                CmdOption::new("eXit", "EXIT"),
            ],
            ShellStep::BodyOption => vec![
                CmdOption::new("Shell", "SHELL"),
                CmdOption::new("eXit", "EXIT"),
            ],
            ShellStep::RemoveFaces => Self::face_options(false),
            ShellStep::AddFaces => Self::face_options(true),
            _ => Vec::new(),
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(
            self.step,
            ShellStep::SelectSolid | ShellStep::RemoveFaces | ShellStep::AddFaces
        )
    }

    fn entity_pick_includes_fills(&self) -> bool {
        true
    }

    fn entity_pick_uses_surface_point(&self) -> bool {
        true
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match self.step {
            ShellStep::SelectSolid => {
                self.target = Some(handle);
                self.step = ShellStep::RemoveFaces;
            }
            ShellStep::RemoveFaces if self.target == Some(handle) => {
                self.actions.push(ShellFaceAction::Remove(point));
            }
            ShellStep::AddFaces if self.target == Some(handle) => {
                self.actions.push(ShellFaceAction::Add(point));
            }
            _ => {}
        }
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let value = text.trim();
        let keyword = value.to_ascii_uppercase();
        let result = match self.step {
            ShellStep::SolidEditOption => match keyword.as_str() {
                "B" | "BODY" => {
                    self.step = ShellStep::BodyOption;
                    CmdResult::NeedPoint
                }
                "X" | "EXIT" => CmdResult::Cancel,
                _ => return Some(CmdResult::NeedPoint),
            },
            ShellStep::BodyOption => match keyword.as_str() {
                "S" | "SHELL" => {
                    self.step = if self.target.is_some() {
                        ShellStep::RemoveFaces
                    } else {
                        ShellStep::SelectSolid
                    };
                    CmdResult::NeedPoint
                }
                "X" | "EXIT" => CmdResult::Cancel,
                _ => return Some(CmdResult::NeedPoint),
            },
            ShellStep::RemoveFaces => match keyword.as_str() {
                "U" | "UNDO" => return Some(self.undo_face_action()),
                "A" | "ADD" => {
                    self.step = ShellStep::AddFaces;
                    CmdResult::NeedPoint
                }
                "ALL" => {
                    self.actions.push(ShellFaceAction::RemoveAll);
                    CmdResult::NeedPoint
                }
                _ => return Some(CmdResult::NeedPoint),
            },
            ShellStep::AddFaces => match keyword.as_str() {
                "U" | "UNDO" => return Some(self.undo_face_action()),
                "R" | "REMOVE" => {
                    self.step = ShellStep::RemoveFaces;
                    CmdResult::NeedPoint
                }
                "ALL" => {
                    self.actions.push(ShellFaceAction::AddAll);
                    CmdResult::NeedPoint
                }
                _ => return Some(CmdResult::NeedPoint),
            },
            ShellStep::Distance => {
                let Ok(distance) = value.parse::<f64>() else {
                    return Some(CmdResult::ReportError(
                        crate::t!("A numeric shell offset distance is required.").into_owned(),
                    ));
                };
                if !distance.is_finite() || distance.abs() <= f64::EPSILON {
                    return Some(CmdResult::ReportError(
                        crate::t!("Value must be nonzero.").into_owned(),
                    ));
                }
                self.default_distance = distance;
                self.finish(distance)
            }
            ShellStep::SelectSolid => return None,
        };
        Some(result)
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            ShellStep::RemoveFaces | ShellStep::AddFaces => {
                self.step = ShellStep::Distance;
                CmdResult::NeedPoint
            }
            ShellStep::Distance => self.finish(self.default_distance),
            _ => CmdResult::Cancel,
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        matches!(self.step, ShellStep::RemoveFaces | ShellStep::AddFaces)
            .then(|| self.undo_face_action())
    }

    fn input_kind(&self) -> InputKind {
        if matches!(self.step, ShellStep::SelectSolid) {
            InputKind::Point
        } else {
            InputKind::SingleToken
        }
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["SHELL", "SOLIDEDIT"]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_shell_collects_face_edits_and_signed_distance() {
        let handle = Handle::new(42);
        let first = DVec3::new(1.0, 2.0, 3.0);
        let second = DVec3::new(4.0, 5.0, 6.0);
        let mut command = ShellCommand::direct(Some(handle));

        command.on_entity_pick(handle, first);
        command.on_text_input("ADD");
        command.on_entity_pick(handle, second);
        command.on_text_input("UNDO");
        command.on_text_input("ALL");
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));

        assert!(matches!(
            command.on_text_input("-0.5"),
            Some(CmdResult::SolidShell {
                handle: result_handle,
                actions,
                distance: -0.5,
            }) if result_handle == handle
                && actions == vec![ShellFaceAction::Remove(first), ShellFaceAction::AddAll]
        ));
    }

    #[test]
    fn invalid_distance_keeps_the_distance_step_active() {
        let mut command = ShellCommand::direct(Some(Handle::new(7)));
        command.on_enter();

        assert!(matches!(
            command.on_text_input("zero"),
            Some(CmdResult::ReportError(_))
        ));
        assert!(matches!(
            command.on_text_input("0.25"),
            Some(CmdResult::SolidShell { distance: 0.25, .. })
        ));
    }
}
