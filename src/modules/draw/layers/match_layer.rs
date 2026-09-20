// Match Layer tool — ribbon definition + CadCommand implementation.

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    ToolDef {
        id: "LAYMATCH",
        label: "Match Layer",
        icon: IconKind::Svg(include_bytes!(
            "../../../../assets/icons/layers/laymatch.svg"
        )),
        event: ModuleEvent::Command("LAYMATCH".to_string()),
    }
}

// ── CadCommand implementation ─────────────────────────────────────────────

use acadrust::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::scene::model::wire_model::WireModel;

pub struct LayMatchCommand {
    dest_handles: Vec<Handle>,
}

impl LayMatchCommand {
    pub fn new(dest: Vec<Handle>) -> Self {
        Self { dest_handles: dest }
    }
}

impl CadCommand for LayMatchCommand {
    fn name(&self) -> &'static str {
        "LAYMATCH"
    }

    fn prompt(&self) -> String {
        if self.dest_handles.is_empty() {
            crate::t!("LAYMATCH  Select objects to change layer:").into_owned()
        } else {
            crate::t!("LAYMATCH  Select source object to match layer from:").into_owned()
        }
    }

    fn is_selection_gathering(&self) -> bool {
        true
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if handles.is_empty() {
            return CmdResult::NeedPoint;
        }
        if self.dest_handles.is_empty() {
            // Phase 1 complete — store destinations, move to phase 2.
            self.dest_handles = handles;
            CmdResult::NeedPoint
        } else {
            // Phase 2 complete — first handle is the source object.
            let src = handles[0];
            CmdResult::MatchEntityLayer {
                dest: std::mem::take(&mut self.dest_handles),
                src,
            }
        }
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
    fn on_hover_entity(&mut self, _handle: Handle, _pt: DVec3) -> Vec<WireModel> {
        vec![]
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["LAYMATCH", "LAYMCH"]
}); // LayMatchCommand
