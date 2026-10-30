// DIMBREAK — create automatic, object-driven or manual gaps in dimensions.

use codec::{EntityType, Handle};
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
    /// Multiple: the dimensions are gathered like any selection.
    PickDimensions(Vec<Handle>),
    /// The objects that break the dimensions, picked one by one until Enter.
    PickCrossing {
        dimensions: Vec<Handle>,
        multiple: bool,
        objects: Vec<Handle>,
    },
    ManualFirst(Vec<Handle>),
    ManualSecond(Vec<Handle>, DVec3),
}

pub struct DimBreakCommand {
    step: Step,
    picked_entity: Option<EntityType>,
}

fn is_dimension(entity: &EntityType) -> bool {
    matches!(entity, EntityType::Dimension(_))
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
            Step::PickDimensions(_) => t!("DIMBREAK  Select dimensions:").into_owned(),
            Step::PickCrossing {
                multiple: false,
                objects,
                ..
            } if objects.is_empty() => {
                t!("DIMBREAK  Select object to break dimension or [Auto/Manual/Remove] <Auto>:")
                    .into_owned()
            }
            Step::PickCrossing {
                multiple: true,
                objects,
                ..
            } if objects.is_empty() => {
                t!("DIMBREAK  Select object to break dimensions or [Auto/Remove] <Auto>:")
                    .into_owned()
            }
            Step::PickCrossing {
                multiple: false, ..
            } => t!("DIMBREAK  Select object to break dimension:").into_owned(),
            Step::PickCrossing { .. } => {
                t!("DIMBREAK  Select object to break dimensions:").into_owned()
            }
            Step::ManualFirst(_) => t!("DIMBREAK  Specify first break point:").into_owned(),
            Step::ManualSecond(_, _) => t!("DIMBREAK  Specify second break point:").into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match &self.step {
            Step::PickDimension => vec![CmdOption::new("Multiple", "MULTIPLE")],
            Step::PickCrossing {
                multiple, objects, ..
            } if objects.is_empty() => {
                let mut options = vec![CmdOption::new("Auto", "AUTO")];
                if !multiple {
                    options.push(CmdOption::new("Manual", "MANUAL"));
                }
                options.push(CmdOption::new("Remove", "REMOVE"));
                options
            }
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        InputKind::Point
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::PickDimension | Step::PickCrossing { .. })
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::PickDimension | Step::PickCrossing { .. })
    }

    fn inject_before_entity_pick(&self) -> bool {
        matches!(self.step, Step::PickDimension)
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked_entity = Some(entity);
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.step, Step::PickDimensions(_))
    }

    fn selection_entity_filter(&self) -> Option<fn(&EntityType) -> bool> {
        Some(is_dimension)
    }

    fn selection_entities_exclude_locked(&self) -> bool {
        true
    }

    fn inject_selection_entities(&mut self, entities: Vec<crate::command::SelectionEntity>) {
        if let Step::PickDimensions(handles) = &mut self.step {
            *handles = entities
                .into_iter()
                .filter(|selected| is_dimension(&selected.entity))
                .map(|selected| selected.handle)
                .collect();
        }
    }

    fn on_selection_complete(&mut self, _handles: Vec<Handle>) -> CmdResult {
        CmdResult::NeedPoint
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
                self.step = Step::PickCrossing {
                    dimensions: vec![handle],
                    multiple: false,
                    objects: Vec::new(),
                };
                CmdResult::NeedPoint
            }
            Step::PickCrossing {
                dimensions,
                objects,
                ..
            } => {
                if !dimensions.contains(&handle) && !objects.contains(&handle) {
                    objects.push(handle);
                }
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text.trim().trim_start_matches('_').to_ascii_uppercase();
        match &mut self.step {
            Step::PickDimension if matches!(keyword.as_str(), "M" | "MULTIPLE") => {
                self.step = Step::PickDimensions(Vec::new());
                Some(CmdResult::NeedPoint)
            }
            Step::PickCrossing {
                dimensions,
                multiple,
                objects,
            } if objects.is_empty() => match keyword.as_str() {
                "A" | "AUTO" => Some(Self::automatic(dimensions)),
                "M" | "MANUAL" if !*multiple => {
                    self.step = Step::ManualFirst(dimensions.clone());
                    Some(CmdResult::NeedPoint)
                }
                "R" | "REMOVE" => Some(Self::remove(dimensions)),
                _ => None,
            },
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
            // Multiple with nothing selected ends quietly.
            Step::PickDimensions(handles) if handles.is_empty() => CmdResult::Cancel,
            Step::PickDimensions(handles) => {
                self.step = Step::PickCrossing {
                    dimensions: handles.clone(),
                    multiple: true,
                    objects: Vec::new(),
                };
                CmdResult::NeedPoint
            }
            Step::PickCrossing {
                dimensions,
                objects,
                ..
            } if objects.is_empty() => Self::automatic(dimensions),
            Step::PickCrossing {
                dimensions,
                objects,
                ..
            } => CmdResult::EditDimensionBreak {
                dimensions: dimensions.clone(),
                operation: crate::command::DimensionBreakOperation::Objects(objects.clone()),
            },
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
