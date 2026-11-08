use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub fn tool() -> ToolDef {
    ToolDef {
        id: "OPTIONS",
        label: "Options",
        icon: IconKind::Svg(crate::ui::icons::GEAR),
        event: ModuleEvent::Command("OPTIONS".to_string()),
    }
}
