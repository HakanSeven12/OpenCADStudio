use crate::t;

use crate::command::{CadCommand, CmdResult};
use glam::DVec3;

// ── Command implementation ────────────────────────────────────────────────

/// Picks one point and delegates to the inline `BASE <x> <y> <z>` handler,
/// which writes the active space's insertion base point onto the document.
pub struct BaseCommand;

impl BaseCommand {
    pub fn new() -> Self {
        Self
    }
}

impl CadCommand for BaseCommand {
    fn name(&self) -> &'static str {
        "BASE"
    }

    fn prompt(&self) -> String {
        t!("BASE  Specify base point:").into_owned()
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        CmdResult::Dispatch(format!("BASE {} {} {}", pt.x, pt.y, pt.z))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["BASE"] }); // BaseCommand
