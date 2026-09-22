use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/cui_import.svg"));
pub fn tool() -> ToolDef {
    crate::modules::ribbon_command("CUIIMPORT")
}
