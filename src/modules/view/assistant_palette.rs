use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/assistant.svg"));
pub fn tool() -> ToolDef {
    ToolDef {
        id: "ASSISTANT",
        label: "Assistant",
        icon: ICON,
        event: ModuleEvent::Command("ASSISTANT".to_string()),
    }
}
