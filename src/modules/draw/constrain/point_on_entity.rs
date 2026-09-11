//! CENTERPOINT / MIDPOINT / ONCURVE — constraints between one picked point
//! and a whole entity selected *before* the tool runs (same "select first,
//! then invoke" convention `DistanceConstraintCommand`
//! (`src/modules/draw/constrain/value.rs`) already uses for its own target
//! entity). The entity side has no ambiguity (a whole circle, or a whole
//! line whose midpoint/curve is being addressed, not one specific
//! endpoint) so it doesn't need interactive picking; the point side does
//! (which of a line's two endpoints, or some other addressable point), so
//! this command accumulates just that one point pick and hands it back for
//! the host to resolve via `nearest_sketch_point` — the same split
//! `CoincidentConstraintCommand` uses and for the same reason (`CadCommand`
//! has no document access).

use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::sketch_constraints::ConstraintKind;
use acadrust::types::Handle;

pub mod center_point_tool {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "CPCONSTRAINT",
            label: "Center Point",
            icon: IconKind::Svg(include_bytes!(
                "../../../../assets/icons/constrain/center_point.svg"
            )),
            event: ModuleEvent::Command("CPCONSTRAINT".to_string()),
        }
    }
}

pub mod midpoint_tool {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "MPCONSTRAINT",
            label: "Midpoint",
            icon: IconKind::Svg(include_bytes!(
                "../../../../assets/icons/constrain/midpoint.svg"
            )),
            event: ModuleEvent::Command("MPCONSTRAINT".to_string()),
        }
    }
}

pub mod point_on_curve_tool {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "OCCONSTRAINT",
            label: "Point on Curve",
            icon: IconKind::Svg(include_bytes!(
                "../../../../assets/icons/constrain/point_on_curve.svg"
            )),
            event: ModuleEvent::Command("OCCONSTRAINT".to_string()),
        }
    }
}

/// Shared by all three tools above — only `name`/`kind`/`label` differ.
pub struct PointOnEntityConstraintCommand {
    name: &'static str,
    kind: ConstraintKind,
    target: Handle,
    label: &'static str,
}

impl PointOnEntityConstraintCommand {
    pub fn new(
        name: &'static str,
        kind: ConstraintKind,
        target: Handle,
        label: &'static str,
    ) -> Self {
        Self {
            name,
            kind,
            target,
            label,
        }
    }
}

impl CadCommand for PointOnEntityConstraintCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        "Specify point:".to_string()
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        CmdResult::AddPointOnEntityConstraint {
            point: pt,
            target: self.target,
            kind: self.kind,
            label: self.label,
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
inventory::submit!(crate::command::CommandRegistration {
    names: &["CPCONSTRAINT", "MPCONSTRAINT", "OCCONSTRAINT"]
});
