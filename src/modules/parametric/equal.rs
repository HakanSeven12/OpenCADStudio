use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult};
use crate::scene::parametric_constraints::ParametricRef;

use super::HorizontalConstraintCommand;

#[derive(Clone, Copy)]
enum Step {
    /// `Select first object or [Multiple]:`, or `Select first object:`
    /// once Multiple was chosen.
    First { multiple: bool },
    /// `Select second object:` — the one that takes the first's size.
    Second(ParametricRef),
    /// `Select object to make equal to first:` — every pick follows the
    /// first at once, Enter ends the command.
    Others(ParametricRef),
}

/// Interactive front end for the Equal geometric constraint: the first
/// object sets the size, the second (or, with Multiple, each further pick)
/// takes its length or radius. Objects are picked inside the command; a
/// selection made before it starts is not used.
pub struct EqualConstraintCommand {
    step: Step,
    picked_entity: Option<EntityType>,
}

impl EqualConstraintCommand {
    pub const NO_OBJECT: &'static str = "No object found.";
    pub const INVALID_OBJECT: &'static str =
        "Invalid selection for Equal. Select a line, polyline segment, circle or arc.";
    pub const INVALID_LENGTH_OBJECT: &'static str =
        "Invalid selection for Equal. Select a line segment, polyline segment, text, MText, major or minor axis of ellipse or elliptical arc.";
    pub const INVALID_RADIUS_OBJECT: &'static str =
        "Invalid selection for Equal. Select a circle or arc.";
    pub const SAME_OBJECT: &'static str =
        "The object or point is already selected.  Select a different object or constraint point.";

    pub fn new() -> Self {
        Self {
            step: Step::First { multiple: false },
            picked_entity: None,
        }
    }

    /// The whole line, circle or arc, or the picked straight polyline
    /// segment.
    pub fn picked_reference(
        entity: &EntityType,
        handle: Handle,
        point: DVec3,
    ) -> Option<ParametricRef> {
        match entity {
            EntityType::Line(_) | EntityType::Circle(_) | EntityType::Arc(_) => {
                Some(ParametricRef::whole(handle))
            }
            EntityType::LwPolyline(_) | EntityType::Polyline2D(_) => {
                let (source, _, _) =
                    crate::scene::centerline::picked_source(entity, handle, point)?;
                let index = usize::try_from(source.segment_index).ok()?;
                HorizontalConstraintCommand::segment_is_straight(entity, index)
                    .then_some(ParametricRef::segment(handle, index))
            }
            _ => None,
        }
    }

    fn report(message: &str) -> CmdResult {
        CmdResult::ReportError(message.to_string())
    }

    fn finish(first: ParametricRef, others: Vec<ParametricRef>, multiple: bool) -> CmdResult {
        CmdResult::AddEqualConstraint {
            first,
            others,
            multiple,
            label: "Equal constraint",
        }
    }
}

impl CadCommand for EqualConstraintCommand {
    fn name(&self) -> &'static str {
        "GCEQUAL"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::First { multiple: false } => {
                "GCEQUAL  Select first object or [Multiple]:".to_string()
            }
            Step::First { multiple: true } => "GCEQUAL  Select first object:".to_string(),
            Step::Second(_) => "GCEQUAL  Select second object:".to_string(),
            Step::Others(_) => "GCEQUAL  Select object to make equal to first:".to_string(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        matches!(self.step, Step::First { multiple: false })
            .then(|| vec![CmdOption::new("Multiple", "M")])
            .unwrap_or_default()
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::First { multiple: false })
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text.trim().trim_start_matches('_').to_ascii_uppercase();
        if matches!(self.step, Step::First { multiple: false })
            && matches!(keyword.as_str(), "M" | "MULTIPLE")
        {
            self.step = Step::First { multiple: true };
            return Some(CmdResult::NeedPoint);
        }
        None
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn entity_pick_accepts_points(&self) -> bool {
        true
    }

    fn typed_point_picks_entity(&self) -> bool {
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
            return Self::report(Self::NO_OBJECT);
        }
        let Some(entity) = self.picked_entity.take() else {
            return CmdResult::NeedPoint;
        };
        let Some(reference) = Self::picked_reference(&entity, handle, point) else {
            return Self::report(Self::INVALID_OBJECT);
        };
        match self.step {
            Step::First { multiple: false } => {
                self.step = Step::Second(reference);
                CmdResult::NeedPoint
            }
            Step::First { multiple: true } => {
                self.step = Step::Others(reference);
                CmdResult::NeedPoint
            }
            Step::Second(first) | Step::Others(first) if first == reference => {
                Self::report(Self::SAME_OBJECT)
            }
            Step::Second(first) => Self::finish(first, vec![reference], false),
            Step::Others(first) => Self::finish(first, vec![reference], true),
        }
    }

    // An empty-space click, or a typed coordinate with nothing under it.
    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        Self::report(Self::NO_OBJECT)
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            // Multiple ends with its summary line; the host prints it.
            Step::Others(first) => Self::finish(first, Vec::new(), true),
            _ => CmdResult::Cancel,
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["ECONSTRAINT", "GCEQUAL"]
});

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::Line;
    use acadrust::types::Vector3;

    fn line() -> EntityType {
        EntityType::Line(Line::from_points(Vector3::ZERO, Vector3::new(10.0, 0.0, 0.0)))
    }

    #[test]
    fn two_picks_make_the_second_follow_the_first() {
        let mut command = EqualConstraintCommand::new();
        command.inject_picked_entity(line());
        assert!(matches!(
            command.on_entity_pick(Handle::new(7), DVec3::ZERO),
            CmdResult::NeedPoint
        ));
        command.inject_picked_entity(line());
        let CmdResult::AddEqualConstraint {
            first,
            others,
            multiple,
            ..
        } = command.on_entity_pick(Handle::new(8), DVec3::ZERO)
        else {
            panic!("the second pick must create the constraint");
        };
        assert_eq!(first, ParametricRef::whole(Handle::new(7)));
        assert_eq!(others, vec![ParametricRef::whole(Handle::new(8))]);
        assert!(!multiple);
    }

    #[test]
    fn multiple_applies_each_pick_and_ends_on_enter() {
        let mut command = EqualConstraintCommand::new();
        assert!(matches!(command.on_text_input("M"), Some(CmdResult::NeedPoint)));
        command.inject_picked_entity(line());
        command.on_entity_pick(Handle::new(7), DVec3::ZERO);
        command.inject_picked_entity(line());
        let CmdResult::AddEqualConstraint {
            others, multiple, ..
        } = command.on_entity_pick(Handle::new(8), DVec3::ZERO)
        else {
            panic!("a Multiple pick must apply at once");
        };
        assert_eq!(others, vec![ParametricRef::whole(Handle::new(8))]);
        assert!(multiple);
        let CmdResult::AddEqualConstraint { others, .. } = command.on_enter() else {
            panic!("Enter must end the Multiple flow through the host");
        };
        assert!(others.is_empty());
    }
}
