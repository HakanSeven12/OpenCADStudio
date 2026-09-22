// Layers panel toggle — ribbon definition.

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    crate::modules::ribbon_action("LAYERS", ModuleEvent::ToggleLayers)
}
