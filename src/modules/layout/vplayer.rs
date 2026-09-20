// VPLAYER — per-viewport layer freeze/thaw command.
//
// Usage (command line):
//   VPLAYER
//   > F <layer_name>        → freeze layer in active viewport
//   > T <layer_name>        → thaw layer in active viewport
//   > F ALL <layer_name>    → freeze layer in ALL viewports
//   > T ALL <layer_name>    → thaw layer in ALL viewports
//   > Enter                 → exit
//
// Layer names are case-insensitive. Multiple space-separated names are accepted.

use crate::t;
use acadrust::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::scene::model::wire_model::WireModel;

pub struct VplayerCommand {
    vp_handle: Handle,
}

impl VplayerCommand {
    pub fn new(vp_handle: Handle) -> Self {
        Self { vp_handle }
    }
}

impl CadCommand for VplayerCommand {
    fn name(&self) -> &'static str {
        "VPLAYER"
    }

    fn prompt(&self) -> String {
        t!("VPLAYER  F <layer> = Freeze  |  T <layer> = Thaw  |  Enter = Exit").into_owned()
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        if text.is_empty() {
            return Some(CmdResult::Cancel);
        }

        let tokens: Vec<&str> = text.split_whitespace().collect();
        if tokens.is_empty() {
            return None;
        }

        let op = tokens[0].to_uppercase();

        // Check for ALL keyword: "F ALL layer1 layer2" or "T ALL layer1"
        let (all_viewports, layer_start) = if tokens
            .get(1)
            .map(|s| s.to_uppercase().as_str() == "ALL")
            .unwrap_or(false)
        {
            (true, 2)
        } else {
            (false, 1)
        };

        let layer_names: Vec<String> = tokens[layer_start..]
            .iter()
            .map(|s| s.to_string())
            .collect();

        if layer_names.is_empty() {
            return None; // no layer name given — ignore and re-prompt
        }

        // Handle::NULL signals "apply to all viewports" in cmd_result.rs
        let vp_handle = if all_viewports {
            acadrust::Handle::NULL
        } else {
            self.vp_handle
        };

        match op.as_str() {
            "F" | "FREEZE" => Some(CmdResult::VpLayerUpdate {
                vp_handle,
                freeze: layer_names,
                thaw: vec![],
            }),
            "T" | "THAW" => Some(CmdResult::VpLayerUpdate {
                vp_handle,
                freeze: vec![],
                thaw: layer_names,
            }),
            _ => None, // unknown op — ignore
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, _pt: DVec3) -> Option<WireModel> {
        None
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["VPLAYER"]
}); // VplayerCommand
