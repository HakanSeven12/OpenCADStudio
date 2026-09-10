//! CCONSTRAINT — the manual Coincident UI (design doc §6.1's open item):
//! unlike the rest of this module's plain select-and-click constraints,
//! Coincident addresses a *point* on an entity (an endpoint, a circle/arc
//! center), not the whole entity, so it needs two point picks instead of a
//! selection.
//!
//! A `CadCommand` has no document access (see `value.rs`'s module doc
//! comment for the same constraint on `DCONSTRAINT`/`ACONSTRAINT`), so this
//! command only accumulates the two raw points `on_point` receives and hands
//! them back as `CmdResult::AddCoincidentConstraint` — the host resolves
//! each point to a real `SketchRef` (`sketch_constraints::nearest_sketch_point`)
//! and adds the constraint, the same split `ReassociateCenterMark` already
//! uses for a similar reason.

use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub mod coincident_tool {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "CCONSTRAINT",
            label: "Coincident",
            icon: IconKind::Svg(include_bytes!("../../../../assets/icons/constrain/coincident.svg")),
            event: ModuleEvent::Command("CCONSTRAINT".to_string()),
        }
    }
}

#[derive(Default)]
pub struct CoincidentConstraintCommand {
    first: Option<DVec3>,
}

impl CoincidentConstraintCommand {
    pub fn new() -> Self {
        Self::default()
    }
}

impl CadCommand for CoincidentConstraintCommand {
    fn name(&self) -> &'static str {
        "CCONSTRAINT"
    }

    fn prompt(&self) -> String {
        if self.first.is_none() {
            "COINCIDENT  Specify first point:".to_string()
        } else {
            "COINCIDENT  Specify second point:".to_string()
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.first {
            None => {
                self.first = Some(pt);
                CmdResult::NeedPoint
            }
            Some(first) => CmdResult::AddCoincidentConstraint { point_a: first, point_b: pt, label: "Coincident constraint" },
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["CCONSTRAINT"] });
