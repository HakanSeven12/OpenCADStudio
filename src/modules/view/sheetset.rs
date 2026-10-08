use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/sheetset.svg"));
pub fn tool() -> ToolDef {
    ToolDef {
        id: "SHEETSET",
        label: "Sheet Set\nManager",
        icon: ICON,
        event: ModuleEvent::Command("_SSMTOGGLE".to_string()),
    }
}
