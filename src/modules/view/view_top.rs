use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/view_top.svg"));
pub fn tool() -> ToolDef {
    crate::modules::ribbon_command_as("VIEW_TOP", "VIEW TOP")
}
