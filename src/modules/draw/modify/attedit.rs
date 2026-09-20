// ATTEDIT command — pick a block reference with attributes, then open the
// attribute editor dialog on it.
//
// This command only performs the entity pick. Once a block is chosen it
// reports the handle through `attedit_pending_handle`; the command host
// (`command_driver`) ends the command and opens the editor dialog. When a
// suitable block is already selected, the ATTEDIT dispatch opens the dialog
// directly and never starts this command (see `app::commands::inquiry`).

use crate::t;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::scene::model::wire_model::WireModel;

pub struct AtteditCommand {
    /// The picked block reference, once the user clicks one.
    picked: Option<acadrust::Handle>,
}

impl AtteditCommand {
    pub fn new() -> Self {
        Self { picked: None }
    }
}

impl CadCommand for AtteditCommand {
    fn name(&self) -> &'static str {
        "ATTEDIT"
    }

    fn prompt(&self) -> String {
        t!("ATTEDIT  Select block with attributes:").into_owned()
    }

    fn needs_entity_pick(&self) -> bool {
        self.picked.is_none()
    }

    fn on_entity_pick(&mut self, handle: acadrust::Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        // Record the pick and yield: the host reads `attedit_pending_handle`
        // and opens the editor dialog for this block.
        self.picked = Some(handle);
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_preview_wires(&mut self, _pt: DVec3) -> Vec<WireModel> {
        vec![]
    }

    fn attedit_pending_handle(&self) -> Option<acadrust::Handle> {
        self.picked
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["ATTEDIT"]
});
