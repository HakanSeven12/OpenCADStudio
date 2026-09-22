use crate::modules::{IconKind, ModuleEvent, ToolDef};

#[allow(dead_code)]
pub fn tool() -> ToolDef {
    crate::modules::ribbon_command("CHANGELOG")
}
