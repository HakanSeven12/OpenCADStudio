use crate::command::{CadCommand, CmdOption, CmdResult};
use document::Handle;
use glam::DVec3;

inventory::submit!(crate::command::CommandRegistration {
    names: &["BCONSTRUCTION"]
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Convert,
    Revert,
    Show,
    Hide,
}

impl Operation {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "CONVERT" | "C" => Some(Self::Convert),
            "REVERT" | "R" => Some(Self::Revert),
            "SHOW" | "SHOW ALL" | "SHOWALL" | "S" => Some(Self::Show),
            "HIDE" | "HIDE ALL" | "HIDEALL" | "H" => Some(Self::Hide),
            _ => None,
        }
    }
}

pub(crate) struct ConstructionCommand {
    pub document: u64,
    pub definition: Handle,
    pub handles: Vec<Handle>,
    pub operation: Option<Operation>,
}

impl ConstructionCommand {
    fn result(&self, operation: Operation) -> CmdResult {
        CmdResult::SetBlockConstruction {
            document: self.document,
            definition: self.definition,
            handles: self.handles.clone(),
            operation,
        }
    }
}

impl CadCommand for ConstructionCommand {
    fn name(&self) -> &'static str {
        "BCONSTRUCTION"
    }
    fn prompt(&self) -> String {
        if self.handles.is_empty() {
            crate::t!("BCONSTRUCTION  Select geometry or [Convert/Revert/Show/Hide]:").into_owned()
        } else {
            crate::t!("BCONSTRUCTION  Choose [Convert/Revert] <Convert>, or press Enter to apply the selected operation:").into_owned()
        }
    }
    fn options(&self) -> Vec<CmdOption> {
        vec![
            CmdOption::new("Convert", "Convert"),
            CmdOption::new("Revert", "Revert"),
            CmdOption::new("Show All", "Show"),
            CmdOption::new("Hide All", "Hide"),
        ]
    }
    fn is_selection_gathering(&self) -> bool {
        true
    }
    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.handles = handles;
        CmdResult::NeedPoint
    }
    fn on_text_input(&mut self, value: &str) -> Option<CmdResult> {
        let Some(operation) = Operation::parse(value) else {
            return Some(CmdResult::ReportError(
                crate::t!("Choose Convert, Revert, Show or Hide.").into_owned(),
            ));
        };
        self.operation = Some(operation);
        Some(
            if matches!(operation, Operation::Show | Operation::Hide) || !self.handles.is_empty() {
                self.result(operation)
            } else {
                CmdResult::NeedPoint
            },
        )
    }
    fn on_point(&mut self, _: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        if self.handles.is_empty() {
            CmdResult::Cancel
        } else {
            self.result(self.operation.unwrap_or(Operation::Convert))
        }
    }
}
