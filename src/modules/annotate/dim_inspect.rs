// -DIMINSPECT — add or remove inspection data on dimensions from the
// command line. The dimensions are picked between the first question and
// the shape (see the app's DIMINSPECT dispatch); this command asks the rest.

use codec::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::entities::dim_inspect::Shape;
use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_edit.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMINSPECT",
        label: "Inspect",
        icon: ICON,
        event: ModuleEvent::Command("DIMINSPECT".to_string()),
    }
}

/// Separates the fields of the internal apply command.
pub const SEP: char = '\u{1f}';

enum Step {
    Choose,
    Shape,
    Label,
    Rate,
}

pub struct DimInspectCommand {
    step: Step,
    handles: Vec<Handle>,
    shape: Shape,
    label: Option<String>,
}

impl DimInspectCommand {
    /// Starts at "Add inspection data or [Remove]".
    pub fn new() -> Self {
        Self {
            step: Step::Choose,
            handles: Vec::new(),
            shape: Shape::Round,
            label: None,
        }
    }

    /// Asks the shape, label and rate for the picked dimensions.
    pub fn adding(handles: Vec<Handle>) -> Self {
        Self {
            step: Step::Shape,
            ..Self::new()
        }
        .with_handles(handles)
    }

    fn with_handles(mut self, handles: Vec<Handle>) -> Self {
        self.handles = handles;
        self
    }

    fn apply(&self, rate: &str) -> CmdResult {
        let shape = match self.shape {
            Shape::Round => "R",
            Shape::Angular => "A",
            Shape::None => "N",
        };
        let handles: Vec<String> = self
            .handles
            .iter()
            .map(|h| format!("{:X}", h.value()))
            .collect();
        CmdResult::Dispatch(format!(
            "_DIMINSPECT_APPLY {shape}{SEP}{}{SEP}{rate}{SEP}{}",
            self.label.as_deref().unwrap_or(""),
            handles.join(",")
        ))
    }
}

impl CadCommand for DimInspectCommand {
    fn name(&self) -> &'static str {
        "-DIMINSPECT"
    }

    fn prompt(&self) -> String {
        let prompt = match self.step {
            Step::Choose => crate::tr!("dim-inspect", "add-or-remove"),
            Step::Shape => crate::tr!("dim-inspect", "shape-prompt"),
            Step::Label => crate::tr!("dim-inspect", "label-prompt"),
            Step::Rate => crate::tr!("dim-inspect", "rate-prompt"),
        };
        format!("-DIMINSPECT  {prompt}")
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Choose => vec![CmdOption::new("Remove", "REMOVE")],
            Step::Shape => vec![
                CmdOption::new("Round", "ROUND"),
                CmdOption::new("Angular", "ANGULAR"),
                CmdOption::new("None", "NONE"),
            ],
            Step::Label | Step::Rate => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Label | Step::Rate => InputKind::FreeText,
            Step::Choose | Step::Shape => InputKind::SingleToken,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let value = text.trim();
        let keyword = value.trim_start_matches(['_', '.']).to_ascii_uppercase();
        match self.step {
            Step::Choose => match keyword.as_str() {
                "A" | "AD" | "ADD" => Some(CmdResult::Dispatch("_DIMINSPECT_SELECT ADD".into())),
                "R" | "RE" | "REM" | "REMO" | "REMOV" | "REMOVE" => {
                    Some(CmdResult::Dispatch("_DIMINSPECT_SELECT REMOVE".into()))
                }
                _ => None,
            },
            Step::Shape => {
                self.shape = match keyword.as_str() {
                    "R" | "RO" | "ROU" | "ROUN" | "ROUND" => Shape::Round,
                    "A" | "AN" | "ANG" | "ANGU" | "ANGUL" | "ANGULA" | "ANGULAR" => Shape::Angular,
                    "N" | "NO" | "NON" | "NONE" => Shape::None,
                    _ => return None,
                };
                self.step = Step::Label;
                Some(CmdResult::NeedPoint)
            }
            Step::Label => {
                self.label = (!value.is_empty()).then(|| value.to_string());
                self.step = Step::Rate;
                Some(CmdResult::NeedPoint)
            }
            Step::Rate => Some(self.apply(if value.is_empty() { "100%" } else { value })),
        }
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Choose => CmdResult::Dispatch("_DIMINSPECT_SELECT ADD".into()),
            Step::Shape => {
                self.step = Step::Label;
                CmdResult::NeedPoint
            }
            Step::Label => {
                self.label = None;
                self.step = Step::Rate;
                CmdResult::NeedPoint
            }
            Step::Rate => self.apply("100%"),
        }
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["DIMINSPECT", "-DIMINSPECT"]
});
