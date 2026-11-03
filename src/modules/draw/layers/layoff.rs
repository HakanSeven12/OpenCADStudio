use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    ToolDef {
        id: "LAYOFF",
        label: "Layer Off",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/layers/layoff.svg")),
        event: ModuleEvent::Command("LAYOFF".to_string()),
    }
}

// ── CadCommand implementation ─────────────────────────────────────────────

use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult};

/// LAYOFF without a preselection: the layer of each picked object is turned
/// off at once, and the command stays for the next pick until Enter or Esc.
/// Undo turns the last one back on.
#[derive(Default)]
pub struct LayerOffCommand {
    /// The layer of the object being picked (injected before the pick).
    picked: Option<String>,
    /// The layers this command turned off, last one last.
    turned_off: Vec<String>,
}

impl CadCommand for LayerOffCommand {
    fn name(&self) -> &'static str {
        "LAYOFF"
    }

    fn prompt(&self) -> String {
        crate::t!("LAYOFF  Select an object on the layer to be turned off or [Undo]:").into_owned()
    }

    fn options(&self) -> Vec<CmdOption> {
        vec![CmdOption::new("Undo", "UNDO")]
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: codec::EntityType) {
        self.picked = Some(entity.common().layer.clone());
    }

    fn on_entity_pick(&mut self, handle: codec::Handle, _pt: DVec3) -> CmdResult {
        let Some(layer) = self.picked.take().filter(|_| !handle.is_null()) else {
            return CmdResult::NeedPoint;
        };
        self.turned_off.push(layer.clone());
        CmdResult::SetLayerOn { layer, on: false }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if !matches!(text.trim().to_uppercase().as_str(), "U" | "UNDO") {
            return None;
        }
        Some(match self.turned_off.pop() {
            Some(layer) => CmdResult::SetLayerOn { layer, on: true },
            None => CmdResult::NeedPoint,
        })
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
