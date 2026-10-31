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

/// LAYCUR: the selected objects move to the current layer.
pub fn laycur_tool() -> ToolDef {
    ToolDef {
        id: "LAYCUR",
        label: "Change to Current Layer",
        icon: IconKind::Svg(include_bytes!(
            "../../../../assets/icons/layers/laycur.svg"
        )),
        event: ModuleEvent::Command("LAYCUR".to_string()),
    }
}

/// COPYTOLAYER: copies of the selected objects go to another layer.
pub fn copy_to_layer_tool() -> ToolDef {
    ToolDef {
        id: "COPYTOLAYER",
        label: "Copy to Layer",
        icon: IconKind::Svg(include_bytes!(
            "../../../../assets/icons/layers/copytolayer.svg"
        )),
        event: ModuleEvent::Command("COPYTOLAYER".to_string()),
    }
}

// ── CadCommand implementation ─────────────────────────────────────────────

use codec::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::scene::model::wire_model::WireModel;

pub struct LayMatchCommand {
    dest_handles: Vec<Handle>,
    /// COPYTOLAYER: copies of the objects go to the layer; they stay put.
    copy: bool,
}

impl LayMatchCommand {
    pub fn new(dest: Vec<Handle>) -> Self {
        Self {
            dest_handles: dest,
            copy: false,
        }
    }

    pub fn copy_to_layer(dest: Vec<Handle>) -> Self {
        Self {
            dest_handles: dest,
            copy: true,
        }
    }
}

impl CadCommand for LayMatchCommand {
    fn name(&self) -> &'static str {
        if self.copy {
            "COPYTOLAYER"
        } else {
            "LAYMATCH"
        }
    }

    fn prompt(&self) -> String {
        if self.copy {
            return if self.dest_handles.is_empty() {
                crate::t!("COPYTOLAYER  Select objects to copy:").into_owned()
            } else {
                crate::t!("COPYTOLAYER  Select an object on the destination layer:").into_owned()
            };
        }
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
                copy: self.copy,
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
    names: &["LAYMATCH", "LAYMCH", "LAYCUR", "COPYTOLAYER"]
}); // LayMatchCommand
