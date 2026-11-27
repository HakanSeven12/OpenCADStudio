use crate::modules::{IconKind, ModuleEvent, ToolDef};
/// View › Palettes › Files: a folder's drawings, to open or insert.
pub fn tool() -> ToolDef {
    ToolDef {
        id: "FILESPALETTE",
        label: "Files",
        icon: IconKind::Svg(crate::icon_assets::FOLDER_TREE),
        event: ModuleEvent::Command("FILESPALETTE".to_string()),
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["FILESPALETTE", "FILESPALETTECLOSE"]
});
