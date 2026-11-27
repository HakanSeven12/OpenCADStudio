// COPYBASE and CUTBASE — copy (or cut) the current selection to the clipboard
// using a base point the user picks (rather than the selection's lower-left
// corner, as COPYCLIP and CUTCLIP do). The command only collects the base
// point; the host copies (and for CUTBASE erases) when it receives the
// dispatched `COPYBASE_AT <x> <y> <z>` or `CUTBASE_AT <x> <y> <z>` token.

use crate::t;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};

pub struct CopyBaseCommand {
    /// CUTBASE: the host erases the objects after it copies them.
    cut: bool,
}

impl Default for CopyBaseCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl CopyBaseCommand {
    pub fn new() -> Self {
        Self { cut: false }
    }

    /// CUTBASE: the same base point step, then the host cuts.
    pub fn cut() -> Self {
        Self { cut: true }
    }
}

impl CadCommand for CopyBaseCommand {
    fn name(&self) -> &'static str {
        if self.cut {
            "CUTBASE"
        } else {
            "COPYBASE"
        }
    }

    fn prompt(&self) -> String {
        if self.cut {
            t!("CUTBASE  Specify base point:").into_owned()
        } else {
            t!("COPYBASE  Specify base point:").into_owned()
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        CmdResult::Dispatch(format!("{}_AT {} {} {}", self.name(), pt.x, pt.y, pt.z))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["COPYBASE", "CUTBASE"]
}); // CopyBaseCommand
