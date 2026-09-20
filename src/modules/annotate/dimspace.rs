// DIMSPACE command — adjust the spacing between parallel linear/aligned dimensions.
//
// Workflow:
//   1. Select the base dimension
//   2. Select the other dimensions to space (click each, Enter to finish)
//   3. Enter a non-negative spacing value, or choose Auto

use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, SelectionEntity};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_space.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMSPACE",
        label: "Dim Space",
        icon: ICON,
        event: ModuleEvent::Command("DIMSPACE".to_string()),
    }
}

enum Step {
    PickBase,
    PickOthers { base: Handle, others: Vec<Handle> },
    EnterSpacing { base: Handle, others: Vec<Handle> },
}

pub struct DimSpaceCommand {
    step: Step,
    picked_entity: Option<EntityType>,
}

impl DimSpaceCommand {
    pub fn new() -> Self {
        Self {
            step: Step::PickBase,
            picked_entity: None,
        }
    }
}

impl CadCommand for DimSpaceCommand {
    fn name(&self) -> &'static str {
        "DIMSPACE"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::PickBase => t!("DIMSPACE  Select base dimension:").into_owned(),
            Step::PickOthers { others, .. } => t!(
                "DIMSPACE  Select dimension to space (%{count} selected, Enter when done):",
                count = others.len()
            )
            .into_owned(),
            Step::EnterSpacing { .. } => {
                t!("DIMSPACE  Specify distance between dimensions or [Auto] <Auto>:").into_owned()
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        matches!(self.step, Step::EnterSpacing { .. })
            .then(|| vec![CmdOption::new("Auto", "AUTO")])
            .unwrap_or_default()
    }

    fn input_kind(&self) -> InputKind {
        if matches!(self.step, Step::EnterSpacing { .. }) {
            InputKind::SingleToken
        } else {
            InputKind::Point
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::PickBase)
    }

    fn inject_before_entity_pick(&self) -> bool {
        matches!(self.step, Step::PickBase)
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked_entity = Some(entity);
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.step, Step::PickOthers { .. })
    }

    fn selection_entities_exclude_locked(&self) -> bool {
        true
    }

    fn inject_selection_entities(&mut self, entities: Vec<SelectionEntity>) {
        let Step::PickOthers { base, others } = &mut self.step else {
            return;
        };
        others.clear();
        for selected in entities {
            if selected.handle != *base
                && matches!(
                    selected.entity,
                    EntityType::Dimension(
                        acadrust::entities::Dimension::Linear(_)
                            | acadrust::entities::Dimension::Aligned(_)
                    )
                )
                && !others.contains(&selected.handle)
            {
                others.push(selected.handle);
            }
        }
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match &mut self.step {
            Step::PickBase => {
                if !matches!(
                    self.picked_entity.take(),
                    Some(EntityType::Dimension(
                        acadrust::entities::Dimension::Linear(_)
                            | acadrust::entities::Dimension::Aligned(_)
                    ))
                ) {
                    return CmdResult::ReportError(
                        t!("DIMSPACE: select a linear or aligned dimension.").into_owned(),
                    );
                }
                self.step = Step::PickOthers {
                    base: handle,
                    others: vec![],
                };
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_selection_complete(&mut self, _handles: Vec<Handle>) -> CmdResult {
        let Step::PickOthers { base, others } = &self.step else {
            return CmdResult::Cancel;
        };
        if others.is_empty() {
            return CmdResult::ReportError(
                t!("DIMSPACE: select one or more linear or aligned dimensions.").into_owned(),
            );
        }
        let base = *base;
        let others = others.clone();
        self.step = Step::EnterSpacing { base, others };
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if let Step::EnterSpacing { base, others } = &self.step {
            let token = text.trim();
            let spacing = if matches!(token.to_ascii_uppercase().as_str(), "A" | "AUTO") {
                None
            } else {
                let Ok(value) = token.parse::<f64>() else {
                    return Some(CmdResult::ReportError(
                        t!("DIMSPACE: enter a non-negative spacing value or Auto.").into_owned(),
                    ));
                };
                if value < 0.0 {
                    return Some(CmdResult::ReportError(
                        t!("DIMSPACE: spacing value cannot be negative.").into_owned(),
                    ));
                }
                Some(value)
            };
            return Some(CmdResult::SpaceDimensions {
                base: *base,
                others: others.clone(),
                spacing,
            });
        }
        None
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if matches!(self.step, Step::EnterSpacing { .. }) {
            return self.on_text_input("AUTO").unwrap_or(CmdResult::Cancel);
        }
        CmdResult::Cancel
    }
}
inventory::submit!(crate::command::CommandRegistration {
    names: &["DIMSPACE", "DSPACE"]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_spacing_returns_a_typed_edit() {
        let mut command = DimSpaceCommand {
            step: Step::EnterSpacing {
                base: Handle::from(2),
                others: vec![Handle::from(3)],
            },
            picked_entity: None,
        };
        let Some(CmdResult::SpaceDimensions {
            base,
            others,
            spacing,
        }) = command.on_text_input("Auto")
        else {
            panic!("expected dimension spacing edit");
        };
        assert_eq!(base, Handle::from(2));
        assert_eq!(others, vec![Handle::from(3)]);
        assert_eq!(spacing, None);
    }
}
