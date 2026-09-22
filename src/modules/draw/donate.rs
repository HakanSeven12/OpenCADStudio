use crate::modules::{IconKind, ModuleEvent, ToolDef};

// Kept for the command-line route ("DONATE" cmd) and possible future ribbon
// re-introduction; currently the Start tab invokes the same command directly.
#[allow(dead_code)]
pub fn tool() -> ToolDef {
    crate::modules::ribbon_command("DONATE")
}
