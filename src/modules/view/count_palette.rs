use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/data_extract.svg"));
/// View › Palettes › Count: opens or closes the Count palette.
pub fn tool() -> ToolDef {
    ToolDef {
        id: "COUNTLIST",
        label: "Count",
        icon: ICON,
        event: ModuleEvent::Command("_COUNTLISTTOGGLE".to_string()),
    }
}
