// -XREF — the command-line form of the reference manager.
//
//   Enter an option [?/Bind/Detach/Path/pathType/Unload/Reload/Overlay/Attach/Show] <Attach>:
//
// Each option collects its answers with the reference prompts and hands them
// to the existing reference operations through `CmdResult::Dispatch`.

use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Option,
    List,
    Names(&'static str),
    File { overlay: bool },
    PathNames,
    NewPath { name: String },
    PathTypeNames,
    PathTypeKind { names: String },
    ShowNames,
}

pub struct XrefCommand {
    step: Step,
    /// Saved paths by reference name, for the Path option's "Old path:" line.
    saved_paths: Vec<(String, String)>,
}

impl XrefCommand {
    pub fn new(saved_paths: Vec<(String, String)>) -> Self {
        Self {
            step: Step::Option,
            saved_paths,
        }
    }

    fn choose(&mut self, token: &str) -> CmdResult {
        self.step = match token {
            "?" => Step::List,
            "B" | "BIND" => Step::Names("Bind"),
            "D" | "DETACH" => Step::Names("Detach"),
            "P" | "PATH" => Step::PathNames,
            "T" | "PATHTYPE" => Step::PathTypeNames,
            "U" | "UNLOAD" => Step::Names("Unload"),
            "R" | "RELOAD" => Step::Names("Reload"),
            "O" | "OVERLAY" => Step::File { overlay: true },
            "A" | "ATTACH" => Step::File { overlay: false },
            "S" | "SHOW" => Step::ShowNames,
            _ => {
                return CmdResult::ReportError("Invalid option keyword.".to_string());
            }
        };
        CmdResult::NeedPoint
    }
}

impl CadCommand for XrefCommand {
    fn name(&self) -> &'static str {
        "-XREF"
    }

    fn prompt(&self) -> String {
        let text = match &self.step {
            Step::Option => "Enter an option [?/Bind/Detach/Path/pathType/Unload/Reload/Overlay/Attach/Show] <Attach>:",
            Step::List => "Enter xref name(s) to list <*>:",
            Step::Names("Bind") => "Enter xref name(s) to bind:",
            Step::Names("Detach") => "Enter xref name(s) to detach:",
            Step::Names("Unload") => "Enter xref name(s) to unload:",
            Step::Names(_) => "Enter xref name(s) to reload:",
            Step::File { overlay: false } => "Enter name of file to attach:",
            Step::File { overlay: true } => "Enter name of file to overlay:",
            Step::PathNames => "Edit xref name(s) to edit path:",
            Step::NewPath { .. } => "Enter new path:",
            Step::PathTypeNames => "Enter xref name(s) to edit path type:",
            Step::PathTypeKind { .. } => "Enter new path type [Full/Relative/None]:",
            Step::ShowNames => "Enter xref name(s) to show:",
        };
        format!("-XREF  {text}")
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Option => vec![
                CmdOption::new("?", "?"),
                CmdOption::new("Bind", "B"),
                CmdOption::new("Detach", "D"),
                CmdOption::new("Path", "P"),
                CmdOption::new("pathType", "T"),
                CmdOption::new("Unload", "U"),
                CmdOption::new("Reload", "R"),
                CmdOption::new("Overlay", "O"),
                CmdOption::new("Attach", "A"),
                CmdOption::new("Show", "S"),
            ],
            Step::PathTypeKind { .. } => vec![
                CmdOption::new("Full", "F"),
                CmdOption::new("Relative", "R"),
                CmdOption::new("None", "N"),
            ],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            // Paths and names can hold spaces.
            Step::File { .. } | Step::NewPath { .. } => InputKind::FreeText,
            _ => InputKind::SingleToken,
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let value = text.trim().trim_matches('"').to_string();
        let token = value.to_uppercase();
        Some(match self.step.clone() {
            Step::Option => self.choose(&token),
            Step::List => CmdResult::Dispatch(format!("XREFLIST {value}")),
            Step::Names(op) => CmdResult::Dispatch(format!("XREF {op} {value}")),
            Step::File { overlay } => {
                let verb = if overlay { "XREFOVERLAYFILE" } else { "XREFATTACHFILE" };
                CmdResult::Dispatch(format!("{verb} {value}"))
            }
            Step::PathNames => {
                let Some((name, old)) = self
                    .saved_paths
                    .iter()
                    .find(|(name, _)| crate::io::xref_model::wildcard_match(name, &value))
                    .cloned()
                else {
                    return Some(CmdResult::CancelWithMessage(
                        "No matching xref names found.".to_string(),
                    ));
                };
                self.step = Step::NewPath { name: name.clone() };
                CmdResult::ReportMeasurement(format!(
                    "{}\n{}",
                    format!("xref name: \"{}\"", name),
                    format!("Old path: {}", display_path(&old))
                ))
            }
            Step::NewPath { name } => CmdResult::Dispatch(format!("XREF Path {name} {value}")),
            Step::PathTypeNames => {
                self.step = Step::PathTypeKind { names: value };
                CmdResult::NeedPoint
            }
            Step::PathTypeKind { names } => {
                let kind = match token.as_str() {
                    "F" | "FULL" => "Full",
                    "R" | "RELATIVE" => "Relative",
                    "N" | "NONE" => "None",
                    _ => {
                        return Some(CmdResult::ReportError(
                            "Invalid option keyword.".to_string(),
                        ))
                    }
                };
                CmdResult::Dispatch(format!("XREF Pathtype {kind} {names}"))
            }
            Step::ShowNames => CmdResult::Dispatch("XREFSHOW".to_string()),
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step.clone() {
            Step::Option => self.choose("A"),
            Step::List => CmdResult::Dispatch("XREFLIST *".to_string()),
            Step::NewPath { .. } => {
                CmdResult::CancelWithMessage("Path unchanged.".to_string())
            }
            Step::PathTypeKind { .. } => CmdResult::NeedPoint,
            _ => CmdResult::Cancel,
        }
    }
}

/// A stored path shown the way the platform writes it.
pub fn display_path(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::XrefCommand;
    use crate::command::{CadCommand, CmdResult};

    #[test]
    fn enter_chooses_attach() {
        let mut cmd = XrefCommand::new(Vec::new());
        assert!(matches!(cmd.on_enter(), CmdResult::NeedPoint));
        assert!(cmd.prompt().contains("Enter name of file to attach:"));
    }

    #[test]
    fn list_defaults_to_every_reference() {
        let mut cmd = XrefCommand::new(Vec::new());
        cmd.on_text_input("?");
        assert!(cmd.prompt().contains("Enter xref name(s) to list <*>:"));
        assert!(matches!(cmd.on_enter(), CmdResult::Dispatch(ref s) if s == "XREFLIST *"));
    }

    #[test]
    fn overlay_file_is_dispatched() {
        let mut cmd = XrefCommand::new(Vec::new());
        cmd.on_text_input("O");
        assert!(cmd.prompt().contains("Enter name of file to overlay:"));
        assert!(matches!(
            cmd.on_text_input("C:/refs/site plan.dwg"),
            Some(CmdResult::Dispatch(ref s)) if s == "XREFOVERLAYFILE C:/refs/site plan.dwg"
        ));
    }
}
