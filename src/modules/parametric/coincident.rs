//! Interactive Coincident input: ordered point/curve picks, repeated
//! point-to-curve placement, and Coincident-only automatic inference.

use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, CoincidentPick};
use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub mod coincident_tool {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "CCONSTRAINT",
            label: "Coincident",
            icon: IconKind::Svg(include_bytes!(
                "../../../assets/icons/constrain/coincident.svg"
            )),
            event: ModuleEvent::Command("CCONSTRAINT".to_string()),
        }
    }
}

#[derive(Clone)]
enum Step {
    First,
    FirstObject,
    Second(CoincidentPick),
    PointsOnCurve {
        curve: CoincidentPick,
        multiple: bool,
    },
    AutoConstrain {
        handles: Vec<Handle>,
    },
}

pub struct CoincidentConstraintCommand {
    step: Step,
    picked_entity: Option<EntityType>,
}

impl CoincidentConstraintCommand {
    pub fn new() -> Self {
        Self {
            step: Step::First,
            picked_entity: None,
        }
    }

    fn point_pick(handle: Option<Handle>, point: DVec3) -> CoincidentPick {
        CoincidentPick {
            handle,
            point,
            whole_curve: false,
        }
    }

    fn curve_pick(handle: Handle, point: DVec3) -> CoincidentPick {
        CoincidentPick {
            handle: Some(handle),
            point,
            whole_curve: true,
        }
    }

    fn picked_constraint_point(entity: &EntityType, point: DVec3) -> bool {
        crate::scene::parametric_constraints::is_parametric_point_near(
            entity,
            acadrust::types::Vector3::new(point.x, point.y, point.z),
        )
    }

    fn valid_curve(entity: &EntityType) -> bool {
        matches!(
            entity,
            EntityType::Line(_)
                | EntityType::LwPolyline(_)
                | EntityType::Polyline2D(_)
                | EntityType::Circle(_)
                | EntityType::Arc(_)
                | EntityType::Ellipse(_)
                | EntityType::Spline(_)
        )
    }
}

impl CadCommand for CoincidentConstraintCommand {
    fn name(&self) -> &'static str {
        "GCCOINCIDENT"
    }

    fn prompt(&self) -> String {
        match self.step.clone() {
            Step::First => {
                "COINCIDENT  Select first point or [Object/Autoconstrain] <Object>:".to_string()
            }
            Step::FirstObject => "COINCIDENT  Select object:".to_string(),
            Step::Second(_) => "COINCIDENT  Select second point or object:".to_string(),
            Step::PointsOnCurve {
                multiple: false, ..
            } => "COINCIDENT  Select point or [Multiple]:".to_string(),
            Step::PointsOnCurve { multiple: true, .. } => {
                "COINCIDENT  Select points to coincide with the first object (Enter = done):"
                    .to_string()
            }
            Step::AutoConstrain { handles } if handles.is_empty() => {
                "COINCIDENT  Select objects:".to_string()
            }
            Step::AutoConstrain { handles } => format!(
                "COINCIDENT  Select objects ({} selected, Enter = apply):",
                handles.len()
            ),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &self.step {
            Step::First => {
                self.step = Step::Second(Self::point_pick(None, pt));
                CmdResult::NeedPoint
            }
            Step::FirstObject => CmdResult::NeedPoint,
            Step::Second(first) => CmdResult::AddCoincidentConstraint {
                first: *first,
                second: Self::point_pick(None, pt),
                multiple: false,
                label: "Coincident constraint",
            },
            Step::PointsOnCurve { curve, multiple } => CmdResult::AddCoincidentConstraint {
                first: *curve,
                second: Self::point_pick(None, pt),
                multiple: *multiple,
                label: "Coincident constraint",
            },
            Step::AutoConstrain { .. } => CmdResult::NeedPoint,
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step.clone() {
            Step::First => vec![
                CmdOption::new("Object", "O"),
                CmdOption::new("Autoconstrain", "A"),
            ],
            Step::PointsOnCurve {
                multiple: false, ..
            } => vec![
                CmdOption::new("Point", "P"),
                CmdOption::new("Multiple", "M"),
            ],
            Step::PointsOnCurve { multiple: true, .. } => vec![CmdOption::enter("Done")],
            _ => Vec::new(),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        !matches!(&self.step, Step::AutoConstrain { .. })
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text.trim().trim_start_matches('_').to_ascii_uppercase();
        match (&mut self.step, keyword.as_str()) {
            (Step::First, "O" | "OBJECT") => {
                self.step = Step::FirstObject;
                Some(CmdResult::NeedPoint)
            }
            (Step::First, "A" | "AUTOCONSTRAIN") => {
                self.step = Step::AutoConstrain {
                    handles: Vec::new(),
                };
                Some(CmdResult::NeedPoint)
            }
            (Step::PointsOnCurve { multiple, .. }, "M" | "MULTIPLE") => {
                *multiple = true;
                Some(CmdResult::NeedPoint)
            }
            (Step::PointsOnCurve { .. }, "P" | "POINT") => Some(CmdResult::NeedPoint),
            _ => None,
        }
    }

    fn needs_entity_pick(&self) -> bool {
        !matches!(&self.step, Step::AutoConstrain { .. })
    }

    fn entity_pick_accepts_points(&self) -> bool {
        true
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked_entity = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        let is_point = self
            .picked_entity
            .as_ref()
            .is_some_and(|entity| Self::picked_constraint_point(entity, point));
        let is_curve = self.picked_entity.as_ref().is_some_and(Self::valid_curve);
        self.picked_entity = None;

        match self.step.clone() {
            Step::FirstObject if is_curve => {
                self.step = Step::PointsOnCurve {
                    curve: Self::curve_pick(handle, point),
                    multiple: false,
                };
                CmdResult::NeedPoint
            }
            Step::First if is_point => {
                self.step = Step::Second(Self::point_pick(Some(handle), point));
                CmdResult::NeedPoint
            }
            Step::First if is_curve => {
                self.step = Step::PointsOnCurve {
                    curve: Self::curve_pick(handle, point),
                    multiple: false,
                };
                CmdResult::NeedPoint
            }
            Step::Second(first) if is_point => CmdResult::AddCoincidentConstraint {
                first,
                second: Self::point_pick(Some(handle), point),
                multiple: false,
                label: "Coincident constraint",
            },
            Step::Second(first) if is_curve => CmdResult::AddCoincidentConstraint {
                first,
                second: Self::curve_pick(handle, point),
                multiple: false,
                label: "Coincident constraint",
            },
            Step::PointsOnCurve { curve, multiple } if is_point => {
                CmdResult::AddCoincidentConstraint {
                    first: curve,
                    second: Self::point_pick(Some(handle), point),
                    multiple,
                    label: "Coincident constraint",
                }
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(&self.step, Step::AutoConstrain { .. })
    }

    fn selection_forces_add(&self) -> bool {
        true
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if let Step::AutoConstrain { handles: selected } = &mut self.step {
            *selected = handles;
        }
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if matches!(&self.step, Step::First) {
            self.step = Step::FirstObject;
            return CmdResult::NeedPoint;
        }
        if let Step::AutoConstrain { handles } = &self.step {
            if !handles.is_empty() {
                return CmdResult::AddAutoCoincidentConstraints {
                    handles: handles.clone(),
                };
            }
        }
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["CCONSTRAINT", "GCCOINCIDENT"]
});

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::types::Vector3;

    #[test]
    fn multiple_keeps_the_first_curve_for_repeated_points() {
        let mut command = CoincidentConstraintCommand::new();
        assert!(matches!(
            command.on_text_input("Object"),
            Some(CmdResult::NeedPoint)
        ));
        command.inject_picked_entity(EntityType::Line(acadrust::entities::Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_X,
        )));
        let handle = Handle::new(7);
        assert!(matches!(
            command.on_entity_pick(handle, DVec3::ZERO),
            CmdResult::NeedPoint
        ));
        assert!(matches!(
            command.on_text_input("Multiple"),
            Some(CmdResult::NeedPoint)
        ));

        assert!(matches!(
            command.on_point(DVec3::X),
            CmdResult::AddCoincidentConstraint {
                first: CoincidentPick {
                    handle: Some(found),
                    whole_curve: true,
                    ..
                },
                multiple: true,
                ..
            } if found == handle
        ));
    }

    #[test]
    fn automatic_mode_returns_the_completed_selection() {
        let mut command = CoincidentConstraintCommand::new();
        assert!(matches!(
            command.on_text_input("Autoconstrain"),
            Some(CmdResult::NeedPoint)
        ));
        let handles = vec![Handle::new(3), Handle::new(5)];
        assert!(matches!(
            command.on_selection_complete(handles.clone()),
            CmdResult::NeedPoint
        ));
        assert!(matches!(
            command.on_enter(),
            CmdResult::AddAutoCoincidentConstraints { handles: found } if found == handles
        ));
    }
}
