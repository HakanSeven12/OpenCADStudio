use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/ucs_icon.svg"));
pub fn tool() -> ToolDef {
    ToolDef {
        id: "UCSICON",
        label: "UCS\nIcon",
        icon: ICON,
        // One click flips the icon (the button shows the state); typing
        // UCSICON keeps its On / Off / NoOrigin / Origin prompt.
        event: ModuleEvent::Command("UCSICON TOGGLE".to_string()),
    }
}
