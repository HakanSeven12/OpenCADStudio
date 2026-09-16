//! Ordered entity/segment picking for the persistent Parallel constraint.

use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::scene::parametric_constraints::{
    parallel_constraint_ref_for_pick, ConstraintKind, ParametricRef,
};

const INVALID_SELECTION: &str = "Invalid selection for Parallel. Select a line, straight polyline segment, text, MText, or a major/minor ellipse axis.";

pub struct ParallelConstraintCommand {
    first: Option<ParametricRef>,
    picked_entity: Option<EntityType>,
}

impl ParallelConstraintCommand {
    pub fn new() -> Self {
        Self {
            first: None,
            picked_entity: None,
        }
    }
}

impl CadCommand for ParallelConstraintCommand {
    fn name(&self) -> &'static str {
        "GCPARALLEL"
    }

    fn prompt(&self) -> String {
        if self.first.is_some() {
            "PARALLEL  Select second object:".to_string()
        } else {
            "PARALLEL  Select first object:".to_string()
        }
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn needs_entity_pick(&self) -> bool {
        true
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
            self.picked_entity = None;
            return CmdResult::ReportError(INVALID_SELECTION.to_string());
        }
        let Some(entity) = self.picked_entity.take() else {
            return CmdResult::ReportError(INVALID_SELECTION.to_string());
        };
        let Some(reference) = parallel_constraint_ref_for_pick(&entity, handle, point) else {
            return CmdResult::ReportError(INVALID_SELECTION.to_string());
        };
        let Some(first) = self.first else {
            self.first = Some(reference);
            return CmdResult::NeedPoint;
        };
        if first == reference {
            return CmdResult::ReportError(
                "Parallel requires two different line directions.".to_string(),
            );
        }
        CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Parallel,
            refs: vec![first, reference],
            driving_param: None,
            label: "Parallel constraint",
        }
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["PCONSTRAINT", "GCPARALLEL"]
});
