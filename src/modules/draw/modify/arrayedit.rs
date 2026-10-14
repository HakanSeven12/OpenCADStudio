// ARRAYEDIT — edit an associative array. Counts and spacings are edited in
// Properties; Source opens the objects every item repeats in the Block
// Editor, and closing it updates all items at once.

use crate::command::{CadCommand, CmdOption, CmdResult};
use codec::Handle;
use glam::DVec3;

pub struct ArrayEditCommand {
    array: Option<Handle>,
}

impl ArrayEditCommand {
    pub fn new(array: Option<Handle>) -> Self {
        Self { array }
    }
}

impl CadCommand for ArrayEditCommand {
    fn name(&self) -> &'static str {
        "ARRAYEDIT"
    }

    fn prompt(&self) -> String {
        match self.array {
            None => crate::t!("ARRAYEDIT  Select array:").into_owned(),
            Some(_) => crate::t!("ARRAYEDIT  Enter an option [Source/eXit] <eXit>:").into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.array {
            None => Vec::new(),
            Some(_) => vec![
                CmdOption::new(crate::t!("Source").as_ref(), "S"),
                CmdOption::new(crate::t!("eXit").as_ref(), "X"),
            ],
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.array.is_none()
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.array = Some(handle);
        CmdResult::NeedPoint
    }

    fn wants_text_input(&self) -> bool {
        self.array.is_some()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let array = self.array?;
        match text.trim().trim_start_matches('_').to_ascii_uppercase().as_str() {
            "S" | "SOURCE" => Some(CmdResult::Dispatch(format!(
                "ARRAYEDIT_SOURCE:{}",
                array.value()
            ))),
            "X" | "EXIT" => Some(CmdResult::Cancel),
            _ => None,
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["ARRAYEDIT"] });
