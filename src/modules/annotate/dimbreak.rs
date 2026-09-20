// DIMBREAK — create automatic, object-driven or manual gaps in dimensions.

use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_break.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMBREAK",
        label: "Dim Break",
        icon: ICON,
        event: ModuleEvent::Command("DIMBREAK".to_string()),
    }
}

enum Step {
    PickDimension,
    PickDimensions(Vec<Handle>),
    PickCrossing(Vec<Handle>),
    ManualFirst(Vec<Handle>),
    ManualSecond(Vec<Handle>, DVec3),
}

pub struct DimBreakCommand {
    step: Step,
    picked_entity: Option<EntityType>,
}

impl DimBreakCommand {
    pub fn new() -> Self {
        Self {
            step: Step::PickDimension,
            picked_entity: None,
        }
    }

    fn automatic(handles: &[Handle]) -> CmdResult {
        CmdResult::EditDimensionBreak {
            dimensions: handles.to_vec(),
            operation: crate::command::DimensionBreakOperation::Auto,
        }
    }

    fn remove(handles: &[Handle]) -> CmdResult {
        CmdResult::EditDimensionBreak {
            dimensions: handles.to_vec(),
            operation: crate::command::DimensionBreakOperation::Remove,
        }
    }
}

impl CadCommand for DimBreakCommand {
    fn name(&self) -> &'static str {
        "DIMBREAK"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::PickDimension => {
                t!("DIMBREAK  Select dimension to add/remove break or [Multiple]:").into_owned()
            }
            Step::PickDimensions(handles) => crate::tf!(
                "DIMBREAK  Select dimensions ({} selected, press Enter when done):",
                handles.len()
            )
            .into_owned(),
            Step::PickCrossing(_) => {
                t!("DIMBREAK  Select object to break dimension or [Auto/Manual/Remove] <Auto>:")
                    .into_owned()
            }
            Step::ManualFirst(_) => t!("DIMBREAK  Specify first break point:").into_owned(),
            Step::ManualSecond(_, _) => t!("DIMBREAK  Specify second break point:").into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::PickDimension => vec![CmdOption::new("Multiple", "MULTIPLE")],
            Step::PickCrossing(_) => vec![
                CmdOption::new("Auto", "AUTO"),
                CmdOption::new("Manual", "MANUAL"),
                CmdOption::new("Remove", "REMOVE"),
            ],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        InputKind::Point
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::PickDimension | Step::PickCrossing(_))
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(
            self.step,
            Step::PickDimension | Step::PickDimensions(_) | Step::PickCrossing(_)
        )
    }

    fn inject_before_entity_pick(&self) -> bool {
        matches!(self.step, Step::PickDimension | Step::PickDimensions(_))
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked_entity = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _point: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match &mut self.step {
            Step::PickDimension => {
                if !matches!(self.picked_entity.take(), Some(EntityType::Dimension(_))) {
                    return CmdResult::ReportError(
                        t!("DIMBREAK: select a dimension.").into_owned(),
                    );
                }
                self.step = Step::PickCrossing(vec![handle]);
                CmdResult::NeedPoint
            }
            Step::PickDimensions(handles) => {
                if !matches!(self.picked_entity.take(), Some(EntityType::Dimension(_))) {
                    return CmdResult::ReportError(
                        t!("DIMBREAK: select a dimension.").into_owned(),
                    );
                }
                if !handles.contains(&handle) {
                    handles.push(handle);
                }
                CmdResult::NeedPoint
            }
            Step::PickCrossing(handles) => CmdResult::EditDimensionBreak {
                dimensions: handles.clone(),
                operation: crate::command::DimensionBreakOperation::Object(handle),
            },
            Step::ManualFirst(_) | Step::ManualSecond(_, _) => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text.trim().to_ascii_uppercase();
        match &mut self.step {
            Step::PickDimension if matches!(keyword.as_str(), "M" | "MULTIPLE") => {
                self.step = Step::PickDimensions(Vec::new());
                Some(CmdResult::NeedPoint)
            }
            Step::PickCrossing(handles) if matches!(keyword.as_str(), "A" | "AUTO") => {
                Some(Self::automatic(handles))
            }
            Step::PickCrossing(handles) if matches!(keyword.as_str(), "M" | "MANUAL") => {
                self.step = Step::ManualFirst(handles.clone());
                Some(CmdResult::NeedPoint)
            }
            Step::PickCrossing(handles) if matches!(keyword.as_str(), "R" | "REMOVE") => {
                Some(Self::remove(handles))
            }
            _ => None,
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match &self.step {
            Step::ManualFirst(handles) => {
                self.step = Step::ManualSecond(handles.clone(), point);
                CmdResult::NeedPoint
            }
            Step::ManualSecond(handles, first) => CmdResult::EditDimensionBreak {
                dimensions: handles.clone(),
                operation: crate::command::DimensionBreakOperation::Manual(*first, point),
            },
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match &self.step {
            Step::PickDimensions(handles) if !handles.is_empty() => {
                self.step = Step::PickCrossing(handles.clone());
                CmdResult::NeedPoint
            }
            Step::PickCrossing(handles) => Self::automatic(handles),
            _ => CmdResult::Cancel,
        }
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["DIMBREAK"]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_break_returns_a_typed_edit() {
        let dimensions = vec![Handle::from(7), Handle::from(9)];
        let CmdResult::EditDimensionBreak {
            dimensions: actual,
            operation: crate::command::DimensionBreakOperation::Auto,
        } = DimBreakCommand::automatic(&dimensions)
        else {
            panic!("expected dimension break edit");
        };
        assert_eq!(actual, dimensions);
    }
}
