use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/vports_highlight.svg"));
/// On/off switch for the faint outline around the hovered Model viewport
/// (VPHIGHLIGHT; its strength is VPHIGHLIGHTOPACITY).
pub fn tool() -> ToolDef {
    ToolDef {
        id: "VPHIGHLIGHT",
        label: "Hover\nOutline",
        icon: ICON,
        event: ModuleEvent::Command("VPHIGHLIGHT TOGGLE".to_string()),
    }
}
