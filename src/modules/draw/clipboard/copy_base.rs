// COPYBASE — copy the current selection to the clipboard using a base point the
// user picks (rather than the selection's lower-left corner, as COPYCLIP does).
// The command only collects the base point; the host performs the copy when it
// receives the dispatched `COPYBASE_AT <x> <y> <z>` token.

use crate::t;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};

pub struct CopyBaseCommand;

impl CopyBaseCommand {
    pub fn new() -> Self {
        Self
    }
}

impl CadCommand for CopyBaseCommand {
    fn name(&self) -> &'static str {
        "COPYBASE"
    }

    fn prompt(&self) -> String {
        t!("COPYBASE  Specify base point:").into_owned()
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        CmdResult::Dispatch(format!("COPYBASE_AT {} {} {}", pt.x, pt.y, pt.z))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["COPYBASE"]
}); // CopyBaseCommand
