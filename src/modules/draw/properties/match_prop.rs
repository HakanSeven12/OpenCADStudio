// Match Properties tool — ribbon definition + CadCommand implementation.

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    ToolDef {
        id: "MATCHPROP",
        label: "Match",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/match_prop.svg")),
        event: ModuleEvent::Command("MATCHPROP".to_string()),
    }
}

// ── CadCommand implementation ─────────────────────────────────────────────

use codec::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;

pub struct MatchPropCommand {
    /// Source handle; NULL until phase 1 is complete.
    src: Handle,
}

impl MatchPropCommand {
    pub fn new() -> Self {
        Self { src: Handle::NULL }
    }

    fn phase1_done(&self) -> bool {
        !self.src.is_null()
    }
}

impl CadCommand for MatchPropCommand {
    fn name(&self) -> &'static str {
        "MATCHPROP"
    }

    fn prompt(&self) -> String {
        if !self.phase1_done() {
            crate::t!("MATCHPROP  Select source object:").into_owned()
        } else {
            crate::t!("MATCHPROP  Select destination objects (Enter to finish):").into_owned()
        }
    }

    // ── Phase 1: single entity pick ───────────────────────────────────────

    fn needs_entity_pick(&self) -> bool {
        !self.phase1_done()
    }

    fn entity_pick_includes_fills(&self) -> bool {
        !self.phase1_done()
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        !self.phase1_done()
    }

    fn entity_pick_acquire_previews(&self, scene: &Scene, handle: Handle) -> Vec<WireModel> {
        if self.phase1_done() || handle.is_null() {
            return vec![];
        }
        scene.fill_hover_preview_wires(handle)
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint; // nothing hit, keep waiting
        }
        self.src = handle;
        CmdResult::NeedPoint // move to phase 2 (selection gathering)
    }

    // ── Phase 2: destination selection gathering ──────────────────────────

    fn is_selection_gathering(&self) -> bool {
        self.phase1_done()
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if handles.is_empty() {
            return CmdResult::NeedPoint;
        }
        CmdResult::MatchProperties {
            dest: handles,
            src: self.src,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_hover_entity(&mut self, _handle: Handle, _pt: DVec3) -> Vec<WireModel> {
        vec![]
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["MATCHPROP"] });  // MatchPropCommand
