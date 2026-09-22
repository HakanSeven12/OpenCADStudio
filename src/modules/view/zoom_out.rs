use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/zoom_out.svg"));
pub fn tool() -> ToolDef {
    crate::modules::ribbon_command_as("ZOOM_OUT", "ZOOM OUT")
}
