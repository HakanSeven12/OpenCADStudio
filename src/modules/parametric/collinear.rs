//! Ordered interactive Collinear input.  The first linear object is the
//! reference; each following object moves onto its infinite axis.

use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, CollinearPick};

const INVALID_SELECTION: &str = "Invalid selection for Collinear. Select a line segment, polyline segment, text, MText, major or minor axis of ellipse or elliptical arc.";

#[derive(Clone, Copy)]
enum Step {
    First { multiple: bool },
    Next {
        reference: CollinearPick,
        multiple: bool,
    },
}

pub struct CollinearConstraintCommand {
    step: Step,
    picked_entity: Option<EntityType>,
}

impl CollinearConstraintCommand {
    pub fn new() -> Self {
        Self {
            step: Step::First { multiple: false },
            picked_entity: None,
        }
    }

    fn valid_entity(entity: &EntityType) -> bool {
        matches!(
            entity,
            EntityType::Line(_)
                | EntityType::LwPolyline(_)
                | EntityType::Polyline2D(_)
                | EntityType::Ellipse(_)
                | EntityType::Text(_)
                | EntityType::MText(_)
        )
    }
}

impl CadCommand for CollinearConstraintCommand {
    fn name(&self) -> &'static str {
        "GCCOLLINEAR"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::First { .. } => "COLLINEAR  Select first object or [Multiple]:".to_string(),
            Step::Next { multiple: false, .. } => {
                "COLLINEAR  Select second object or [Multiple]:".to_string()
            }
            Step::Next { multiple: true, .. } => {
                "COLLINEAR  Select objects to make collinear (Enter = done):".to_string()
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::First { .. } | Step::Next { multiple: false, .. } => {
                vec![CmdOption::new("Multiple", "M")]
            }
            Step::Next { multiple: true, .. } => vec![CmdOption::enter("Done")],
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text.trim().trim_start_matches('_').to_ascii_uppercase();
        if !matches!(keyword.as_str(), "M" | "MULTIPLE") {
            return None;
        }
        match &mut self.step {
            Step::First { multiple } | Step::Next { multiple, .. } => *multiple = true,
        }
        Some(CmdResult::NeedPoint)
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
        if handle.is_null()
            || !self
                .picked_entity
                .as_ref()
                .is_some_and(Self::valid_entity)
        {
            self.picked_entity = None;
            return CmdResult::ReportError(INVALID_SELECTION.to_string());
        }
        self.picked_entity = None;
        let pick = CollinearPick { handle, point };
        match self.step {
            Step::First { multiple } => {
                self.step = Step::Next {
                    reference: pick,
                    multiple,
                };
                CmdResult::NeedPoint
            }
            Step::Next {
                reference,
                multiple,
            } if reference.handle != handle => CmdResult::AddCollinearConstraint {
                first: reference,
                second: pick,
                multiple,
                label: "Collinear constraint",
            },
            Step::Next { .. } => CmdResult::ReportError(INVALID_SELECTION.to_string()),
        }
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::ReportError(INVALID_SELECTION.to_string())
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["GCCOLLINEAR", "LCONSTRAINT", "COLLINEAR", "COLINEAR"]
});
