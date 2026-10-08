// DDEDIT — edit the text content of a text-bearing entity in-place.
//
// Workflow:
//   1. Pick a text entity (or fire from double-click / a current selection).
//   2. The picked entity opens its in-place editor: a plain text box for
//      single-line text (Text, attributes, dimension override, tolerance) or
//      the rich MText editor for MText / MultiLeader. A Leader resolves to the
//      entity it annotates.

use codec::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/ddedit.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DDEDIT",
        label: "Edit Text",
        icon: ICON,
        event: ModuleEvent::Command("DDEDIT".to_string()),
    }
}

pub struct DdeditCommand;

impl DdeditCommand {
    pub fn new() -> Self {
        Self
    }
}

impl CadCommand for DdeditCommand {
    fn name(&self) -> &'static str {
        "DDEDIT"
    }

    fn prompt(&self) -> String {
        t!("DDEDIT  Select text entity:").into_owned()
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        // Hand the picked entity to the in-place editor (plain box or rich
        // MText editor, chosen by the entity's type).
        CmdResult::EditTextEntity { handle }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["DDEDIT"] });
