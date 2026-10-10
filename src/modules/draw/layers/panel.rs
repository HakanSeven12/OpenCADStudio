// Layer Manager pallet — ribbon definition.

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    ToolDef {
        id: "LAYERS",
        label: "Layers",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/layers/panel.svg")),
        // A command like the Blocks palette's: it opens the pallet, and the
        // ribbon drops the button's highlight once it has run.
        event: ModuleEvent::Command("LAYERS".to_string()),
    }
}
