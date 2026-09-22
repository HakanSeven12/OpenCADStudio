// Orthographic-projection toggle (Projection ribbon group).
// id "ORTHO" keys the active-state highlight (is_active_tool) off the ribbon's
// projection state; the event uses the PARALLEL verb so the typed ORTHO command
// stays free for the orthogonal cursor-constraint drafting aid.
use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/ortho.svg"));
pub fn tool() -> ToolDef {
    crate::modules::ribbon_command_as("ORTHO", "PARALLEL")
}
