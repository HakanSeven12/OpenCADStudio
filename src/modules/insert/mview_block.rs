use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/blocks/insert.svg"));
pub fn tool() -> ToolDef {
    ToolDef {
        id: "BLOCKPALETTE",
        label: "Block\nPalette",
        icon: ICON,
        event: ModuleEvent::Command("BLOCKPALETTE".to_string()),
    }
}
