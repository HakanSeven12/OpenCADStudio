// ARRAYEDIT — edit an associative array. Counts and spacings are edited in
// Properties; Source opens the objects every item repeats in the Block
// Editor, and closing it updates all items at once. Move item and Erase
// item change single items, which keep that through later edits; Reset
// brings them all back to their places.

use crate::command::{CadCommand, CmdOption, CmdResult};
use document::Handle;
use glam::DVec3;

enum Step {
    Options,
    /// Pick the item to move (`true`) or erase.
    Item(bool),
    MoveBase(DVec3),
    MoveTo(DVec3, DVec3),
}

pub struct ArrayEditCommand {
    array: Option<Handle>,
    step: Step,
}

impl ArrayEditCommand {
    pub fn new(array: Option<Handle>) -> Self {
        Self {
            array,
            step: Step::Options,
        }
    }

    fn dispatch(array: Handle, op: &str, points: &[DVec3]) -> CmdResult {
        let mut line = format!("ARRAYEDIT_ITEM:{}:{op}", array.value());
        for p in points {
            line.push_str(&format!(":{},{},{}", p.x, p.y, p.z));
        }
        CmdResult::Dispatch(line)
    }
}

impl CadCommand for ArrayEditCommand {
    fn name(&self) -> &'static str {
        "ARRAYEDIT"
    }

    fn prompt(&self) -> String {
        if self.array.is_none() {
            return crate::t!("ARRAYEDIT  Select array:").into_owned();
        }
        match self.step {
            Step::Options => crate::t!(
                "ARRAYEDIT  Enter an option [Source/Move item/Erase item/Reset/eXit] <eXit>:"
            )
            .into_owned(),
            Step::Item(_) => crate::t!("ARRAYEDIT  Select item:").into_owned(),
            Step::MoveBase(_) => crate::t!("ARRAYEDIT  Base point:").into_owned(),
            Step::MoveTo(..) => crate::t!("ARRAYEDIT  Second point:").into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match (self.array, &self.step) {
            (Some(_), Step::Options) => vec![
                CmdOption::new(crate::t!("Source").as_ref(), "S"),
                CmdOption::new(crate::t!("Move item").as_ref(), "M"),
                CmdOption::new(crate::t!("Erase item").as_ref(), "E"),
                CmdOption::new(crate::t!("Reset").as_ref(), "R"),
                CmdOption::new(crate::t!("eXit").as_ref(), "X"),
            ],
            _ => Vec::new(),
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
        self.array.is_some() && matches!(self.step, Step::Options)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let array = self.array?;
        match text
            .trim()
            .trim_start_matches('_')
            .to_ascii_uppercase()
            .as_str()
        {
            "S" | "SOURCE" => Some(CmdResult::Dispatch(format!(
                "ARRAYEDIT_SOURCE:{}",
                array.value()
            ))),
            "M" | "MOVE" => {
                self.step = Step::Item(true);
                Some(CmdResult::NeedPoint)
            }
            "E" | "ERASE" => {
                self.step = Step::Item(false);
                Some(CmdResult::NeedPoint)
            }
            "R" | "RESET" => Some(Self::dispatch(array, "R", &[])),
            "X" | "EXIT" => Some(CmdResult::Cancel),
            _ => None,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let Some(array) = self.array else {
            return CmdResult::NeedPoint;
        };
        match self.step {
            Step::Options => CmdResult::NeedPoint,
            Step::Item(true) => {
                self.step = Step::MoveBase(pt);
                CmdResult::NeedPoint
            }
            Step::Item(false) => Self::dispatch(array, "E", &[pt]),
            Step::MoveBase(item) => {
                self.step = Step::MoveTo(item, pt);
                CmdResult::NeedPoint
            }
            Step::MoveTo(item, base) => Self::dispatch(array, "M", &[item, pt - base]),
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["ARRAYEDIT"]
});
