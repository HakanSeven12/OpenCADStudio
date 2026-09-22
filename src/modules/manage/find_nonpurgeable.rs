use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind = IconKind::Svg(include_bytes!(
    "../../../assets/icons/find_nonpurgeable.svg"
));
pub fn tool() -> ToolDef {
    crate::modules::ribbon_command("FINDNONPURGEABLE")
}
