use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/snap_underlays.svg"));
pub fn tool() -> ToolDef {
    crate::modules::ribbon_command("UOSNAP")
}
