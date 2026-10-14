use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/underlay_layers.svg"));
pub fn tool() -> ToolDef {
    ToolDef {
        id: "UNDERLAYLAYERS",
        label: "Underlay\nLayers",
        icon: ICON,
        event: ModuleEvent::Command("ULAYERS".to_string()),
    }
}
