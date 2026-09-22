// DIMEDIT — edit dimension text, rotation, extension-line obliquing or home position.

use acadrust::Handle;
use glam::DVec3;

use crate::command::{
    CadCommand, CmdOption, CmdResult, DimensionEditOperation, InputKind,
};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_edit.svg"));

pub fn tool() -> ToolDef {
    crate::modules::ribbon_command("DIMEDIT")
}

enum Step {
    Choose,
    NewText,
    Rotate,
    Oblique,
    Select(DimensionEditOperation),
}

pub struct DimEditCommand {
    step: Step,
}

impl DimEditCommand {
    pub fn new() -> Self {
        Self { step: Step::Choose }
    }

    fn select(&mut self, operation: DimensionEditOperation) -> CmdResult {
        self.step = Step::Select(operation);
        CmdResult::NeedPoint
    }
}

impl CadCommand for DimEditCommand {
    fn name(&self) -> &'static str {
        "DIMEDIT"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Choose => {
                t!("DIMEDIT  Enter type of dimension editing [Home/New/Rotate/Oblique] <Home>:")
                    .into_owned()
            }
            Step::NewText => t!("DIMEDIT  Enter new dimension text <measured value>:").into_owned(),
            Step::Rotate => t!("DIMEDIT  Specify angle for dimension text:").into_owned(),
            Step::Oblique => {
                t!("DIMEDIT  Enter obliquing angle (press Enter for none):").into_owned()
            }
            Step::Select(_) => t!("DIMEDIT  Select objects:").into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if !matches!(self.step, Step::Choose) {
            return Vec::new();
        }
        vec![
            CmdOption::new("Home", "HOME"),
            CmdOption::new("New", "NEW"),
            CmdOption::new("Rotate", "ROTATE"),
            CmdOption::new("Oblique", "OBLIQUE"),
        ]
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::NewText => InputKind::FreeText,
            Step::Select(_) => InputKind::Point,
            Step::Choose | Step::Rotate | Step::Oblique => InputKind::SingleToken,
        }
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.step, Step::Select(_))
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        let Step::Select(operation) = &self.step else {
            return CmdResult::Cancel;
        };
        if handles.is_empty() {
            CmdResult::Cancel
        } else {
            CmdResult::EditDimensions {
                handles,
                operation: operation.clone(),
            }
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let value = text.trim();
        let keyword = value.to_ascii_uppercase();
        match self.step {
            Step::Choose => match keyword.as_str() {
                "H" | "HOME" => Some(self.select(DimensionEditOperation::Home)),
                "N" | "NEW" => {
                    self.step = Step::NewText;
                    Some(CmdResult::NeedPoint)
                }
                "R" | "ROTATE" => {
                    self.step = Step::Rotate;
                    Some(CmdResult::NeedPoint)
                }
                "O" | "OBLIQUE" => {
                    self.step = Step::Oblique;
                    Some(CmdResult::NeedPoint)
                }
                _ => None,
            },
            Step::NewText => Some(self.select(DimensionEditOperation::NewText(
                if value == "<>" { String::new() } else { text.to_string() },
            ))),
            Step::Rotate => value
                .parse::<f64>()
                .ok()
                .map(|degrees| self.select(DimensionEditOperation::Rotate(degrees))),
            Step::Oblique => value
                .parse::<f64>()
                .ok()
                .filter(|degrees| (-85.0..=85.0).contains(degrees))
                .map(|degrees| self.select(DimensionEditOperation::Oblique(degrees))),
            Step::Select(_) => None,
        }
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Choose => self.select(DimensionEditOperation::Home),
            Step::NewText => self.select(DimensionEditOperation::NewText(String::new())),
            Step::Oblique => self.select(DimensionEditOperation::Oblique(0.0)),
            Step::Rotate | Step::Select(_) => CmdResult::Cancel,
        }
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["DIMEDIT"] });
