// QUICKPRINT / QP — run the command, select objects, then Enter to plot the
// selection's bounding box to a PDF (handled by the host). No dialog. (#325)

use crate::command::{CadCommand, CmdResult};
use codec::Handle;
use glam::DVec3;
use crate::t;

pub struct QuickPrintCommand {
    /// Latest selection set, refreshed on every selection action.
    handles: Vec<Handle>,
}

impl QuickPrintCommand {
    pub fn new() -> Self {
        Self { handles: Vec::new() }
    }
}

impl CadCommand for QuickPrintCommand {
    fn name(&self) -> &'static str {
        "QUICKPRINT"
    }

    fn prompt(&self) -> String {
        if self.handles.is_empty() {
            t!("QUICKPRINT  Select objects to quick-print:").into_owned()
        } else {
            t!(
                "QUICKPRINT  %{count} selected — Enter to plot, or keep selecting:",
                count = self.handles.len()
            )
            .into_owned()
        }
    }

    fn is_selection_gathering(&self) -> bool {
        true
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.handles = handles;
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.handles.is_empty() {
            CmdResult::Cancel
        } else {
            CmdResult::QuickPrint(std::mem::take(&mut self.handles))
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["QUICKPRINT", "QP"]
}); // QuickPrintCommand
