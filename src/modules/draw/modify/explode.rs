// Explode tools — ribbon definition + command implementation. What each
// object breaks into is in `scene::explode`.

pub use crate::scene::explode::*;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::t;
use glam::DVec3;

// ── Ribbon definition ──────────────────────────────────────────────────────

pub fn tool() -> ToolDef {
    ToolDef {
        id: "EXPLODE",
        label: "Explode",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/explode.svg")),
        event: ModuleEvent::Command("EXPLODE".to_string()),
    }
}

pub struct ExplodeCommand;

impl ExplodeCommand {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self
    }
}

impl CadCommand for ExplodeCommand {
    fn name(&self) -> &'static str {
        "EXPLODE"
    }
    fn prompt(&self) -> String {
        t!("EXPLODE  Select objects to explode:").into_owned()
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
