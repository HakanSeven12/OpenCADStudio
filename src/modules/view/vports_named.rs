use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/vports_named.svg"));
pub fn tool() -> ToolDef {
    crate::modules::ribbon_command_as("VPORTS_NAMED", "VPORTS")
}
