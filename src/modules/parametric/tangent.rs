use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::scene::parametric_constraints::ParametricRef;
use crate::t;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TangentOperand {
    Line,
    Circle,
    Ellipse,
}

#[derive(Clone, Copy)]
struct TangentPick {
    reference: ParametricRef,
    operand: TangentOperand,
}

pub struct TangentConstraintCommand {
    first: Option<TangentPick>,
    picked_entity: Option<EntityType>,
}

impl TangentConstraintCommand {
    pub fn new() -> Self {
        Self {
            first: None,
            picked_entity: None,
        }
    }

    fn invalid_selection() -> CmdResult {
        CmdResult::ReportError(
            t!("Invalid selection for Tangent. Select a line, polyline segment, circle, arc or ellipse.")
                .into_owned(),
        )
    }

    fn picked_reference(entity: &EntityType, handle: Handle, point: DVec3) -> Option<TangentPick> {
        let (reference, operand) = match entity {
            EntityType::Line(_) => (ParametricRef::whole(handle), TangentOperand::Line),
            EntityType::Circle(_) | EntityType::Arc(_) => {
                (ParametricRef::whole(handle), TangentOperand::Circle)
            }
            EntityType::Ellipse(_) => (ParametricRef::whole(handle), TangentOperand::Ellipse),
            EntityType::LwPolyline(_) | EntityType::Polyline2D(_) => {
                let (source, _, _) = crate::scene::centerline::picked_source(entity, handle, point)?;
                let index = usize::try_from(source.segment_index).ok()?;
                (ParametricRef::segment(handle, index), TangentOperand::Line)
            }
            _ => return None,
        };
        Some(TangentPick { reference, operand })
    }

    fn pair_supported(first: TangentOperand, second: TangentOperand) -> bool {
        matches!(
            (first, second),
            (TangentOperand::Circle, TangentOperand::Circle)
                | (TangentOperand::Circle, TangentOperand::Line)
                | (TangentOperand::Line, TangentOperand::Circle)
                | (TangentOperand::Ellipse, TangentOperand::Line)
                | (TangentOperand::Line, TangentOperand::Ellipse)
        )
    }

    pub fn preselected_reference(
        entity: &EntityType,
        handle: Handle,
    ) -> Option<ParametricRef> {
        match entity {
            EntityType::Line(_)
            | EntityType::Circle(_)
            | EntityType::Arc(_)
            | EntityType::Ellipse(_) => Some(ParametricRef::whole(handle)),
            EntityType::LwPolyline(polyline) => {
                let segments = if polyline.is_closed {
                    polyline.vertices.len()
                } else {
                    polyline.vertices.len().saturating_sub(1)
                };
                (segments == 1 && polyline.vertices.first()?.bulge.abs() <= 1.0e-12)
                    .then(|| ParametricRef::segment(handle, 0))
            }
            EntityType::Polyline2D(polyline) => {
                let segments = if polyline.is_closed() {
                    polyline.vertices.len()
                } else {
                    polyline.vertices.len().saturating_sub(1)
                };
                (segments == 1 && polyline.vertices.first()?.bulge.abs() <= 1.0e-12)
                    .then(|| ParametricRef::segment(handle, 0))
            }
            _ => None,
        }
    }
}

impl CadCommand for TangentConstraintCommand {
    fn name(&self) -> &'static str {
        "TCONSTRAINT"
    }

    fn prompt(&self) -> String {
        if self.first.is_some() {
            t!("TCONSTRAINT  Select second object:").into_owned()
        } else {
            t!("TCONSTRAINT  Select first object:").into_owned()
        }
    }

    fn needs_entity_pick(&self) -> bool {
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
        let Some(entity) = self.picked_entity.take() else {
            return CmdResult::NeedPoint;
        };
        let Some(pick) = Self::picked_reference(&entity, handle, point) else {
            return Self::invalid_selection();
        };
        let Some(first) = self.first else {
            self.first = Some(pick);
            return CmdResult::NeedPoint;
        };
        if first.reference == pick.reference
            || !Self::pair_supported(first.operand, pick.operand)
        {
            return Self::invalid_selection();
        }
        CmdResult::AddTangentConstraint {
            first: first.reference,
            second: pick.reference,
            label: "Tangent constraint",
        }
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::{Circle, Line};
    use acadrust::types::Vector3;

    #[test]
    fn command_preserves_pick_order_for_the_initial_anchor() {
        let first = Handle::new(11);
        let second = Handle::new(13);
        let mut command = TangentConstraintCommand::new();
        command.inject_picked_entity(EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(4.0, 0.0, 0.0),
        )));
        assert!(matches!(
            command.on_entity_pick(first, DVec3::ZERO),
            CmdResult::NeedPoint
        ));
        command.inject_picked_entity(EntityType::Circle(Circle::from_center_radius(
            Vector3::new(2.0, 3.0, 0.0),
            1.0,
        )));
        let CmdResult::AddTangentConstraint {
            first: first_ref,
            second: second_ref,
            ..
        } = command.on_entity_pick(second, DVec3::ZERO)
        else {
            panic!("second pick must create the constraint");
        };
        assert_eq!(first_ref, ParametricRef::whole(first));
        assert_eq!(second_ref, ParametricRef::whole(second));
    }
}
