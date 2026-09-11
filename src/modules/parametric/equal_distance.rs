//! EDCONSTRAINT makes the distance between one point pair equal the
//! distance between another pair, rather than comparing whole entities.
//! being equal length/radius. Needs four point picks (unlike the two
//! whole-entity selections `ECONSTRAINT` uses), so — like
//! `CoincidentConstraintCommand` — it accumulates raw points and hands them
//! back for the host to resolve (`CadCommand` has no document access).

use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub mod equal_distance_tool {
    use super::*;
    pub fn tool() -> ToolDef {
        ToolDef {
            id: "EDCONSTRAINT",
            label: "Equal Distance",
            icon: IconKind::Svg(include_bytes!(
                "../../../assets/icons/constrain/equal_distance.svg"
            )),
            event: ModuleEvent::Command("EDCONSTRAINT".to_string()),
        }
    }
}

#[derive(Default)]
pub struct EqualDistanceConstraintCommand {
    points: Vec<DVec3>,
}

impl EqualDistanceConstraintCommand {
    pub fn new() -> Self {
        Self::default()
    }
}

impl CadCommand for EqualDistanceConstraintCommand {
    fn name(&self) -> &'static str {
        "EDCONSTRAINT"
    }

    fn prompt(&self) -> String {
        match self.points.len() {
            0 => "EQUAL DISTANCE  Specify first point of first pair:".to_string(),
            1 => "EQUAL DISTANCE  Specify second point of first pair:".to_string(),
            2 => "EQUAL DISTANCE  Specify first point of second pair:".to_string(),
            _ => "EQUAL DISTANCE  Specify second point of second pair:".to_string(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.points.push(pt);
        if self.points.len() < 4 {
            return CmdResult::NeedPoint;
        }
        let [a, b, c, d] = self.points[..] else {
            unreachable!("just checked len == 4")
        };
        CmdResult::AddEqualDistanceConstraint {
            points: [a, b, c, d],
            label: "Equal distance constraint",
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
    names: &["EDCONSTRAINT"]
});
