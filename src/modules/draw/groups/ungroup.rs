// Ungroup tool — ribbon definition + CadCommand implementation.

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    ToolDef {
        id: "UNGROUP",
        label: "Ungroup",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/ungroup.svg")),
        event: ModuleEvent::Command("UNGROUP".to_string()),
    }
}

// ── CadCommand implementation ─────────────────────────────────────────────

use crate::t;
use acadrust::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::scene::model::wire_model::WireModel;

pub struct UngroupCommand;

impl UngroupCommand {
    pub fn new() -> Self {
        Self
    }
}

impl CadCommand for UngroupCommand {
    fn name(&self) -> &'static str {
        "UNGROUP"
    }

    fn prompt(&self) -> String {
        t!("UNGROUP  Select grouped objects:").into_owned()
    }

    fn is_selection_gathering(&self) -> bool {
        true
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if handles.is_empty() {
            return CmdResult::NeedPoint;
        }
        CmdResult::DeleteGroups { handles }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_hover_entity(&mut self, _handle: Handle, _pt: DVec3) -> Vec<WireModel> {
        vec![]
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["UNGROUP"]
}); // UngroupCommand
