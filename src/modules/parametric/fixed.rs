//! GCFIX / FXCONSTRAINT — locks a constraint point, or a whole curve /
//! polyline segment, at its current position. Picks resolve on the host (a
//! `CadCommand` has no document access): `AddFixedConstraint` carries the
//! click, the driver maps it to a `ParametricRef` and re-prompts on a miss,
//! the way the reference does.

use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, CoincidentPick};
use crate::scene::parametric_constraints::ParametricRef;

#[derive(Clone, Copy)]
enum Step {
    PointOrObject,
    Object,
}

pub struct FixConstraintCommand {
    step: Step,
}

impl FixConstraintCommand {
    pub const INVALID_OBJECT: &'static str =
        "Invalid selection for Fixed. Select a line, polyline segment, circle, arc, ellipse or spline.";
    pub const NO_POINT: &'static str = "No valid constraint point found.";
    pub const NO_OBJECT: &'static str = "No object found.";

    pub fn new() -> Self {
        Self {
            step: Step::PointOrObject,
        }
    }

    /// The Object-mode reading of a single preselected entity: a whole
    /// curve, or a one-segment polyline's segment. Anything else prompts.
    pub fn preselected_reference(entity: &EntityType, handle: Handle) -> Option<ParametricRef> {
        match entity {
            EntityType::Line(_)
            | EntityType::Circle(_)
            | EntityType::Arc(_)
            | EntityType::Ellipse(_)
            | EntityType::Spline(_) => Some(ParametricRef::whole(handle)),
            EntityType::LwPolyline(polyline) => (polyline.vertices.len() == 2
                && !polyline.is_closed)
                .then(|| ParametricRef::segment(handle, 0)),
            EntityType::Polyline2D(polyline) => (polyline.vertices.len() == 2
                && !polyline.is_closed())
            .then(|| ParametricRef::segment(handle, 0)),
            _ => None,
        }
    }

    fn pick(&self, handle: Option<Handle>, point: DVec3) -> CmdResult {
        CmdResult::AddFixedConstraint(CoincidentPick {
            handle,
            point,
            whole_curve: matches!(self.step, Step::Object),
        })
    }
}

impl CadCommand for FixConstraintCommand {
    fn name(&self) -> &'static str {
        "FXCONSTRAINT"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::PointOrObject => "FXCONSTRAINT  Select point or [Object] <Object>:".to_string(),
            Step::Object => "FXCONSTRAINT  Select object:".to_string(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        matches!(self.step, Step::PointOrObject)
            .then(|| vec![CmdOption::new("Object", "O")])
            .unwrap_or_default()
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::PointOrObject)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text.trim().trim_start_matches('_').to_ascii_uppercase();
        let to_object =
            matches!(self.step, Step::PointOrObject) && matches!(keyword.as_str(), "O" | "OBJECT");
        to_object.then(|| {
            self.step = Step::Object;
            CmdResult::NeedPoint
        })
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

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        if handle.is_null() {
            return self.on_point(point);
        }
        self.pick(Some(handle), point)
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step {
            Step::PointOrObject => self.pick(None, point),
            // A click on nothing at "Select object:".
            Step::Object => CmdResult::ReportError(Self::NO_OBJECT.to_string()),
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::PointOrObject => {
                self.step = Step::Object;
                CmdResult::NeedPoint
            }
            Step::Object => CmdResult::Cancel,
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
