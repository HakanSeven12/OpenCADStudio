// ByLayer tool — ribbon definition.

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    crate::modules::ribbon_command("BYLAYER")
}
