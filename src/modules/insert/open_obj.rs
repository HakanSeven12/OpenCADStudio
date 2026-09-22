use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub fn tool() -> ToolDef {
    crate::modules::ribbon_action("OPEN", ModuleEvent::OpenFileDialog)
}
