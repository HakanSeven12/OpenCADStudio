use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    ToolDef {
        id: "LAYWALK",
        label: "Layer Walk",
        icon: IconKind::Svg(include_bytes!(
            "../../../../assets/icons/layers/laywalk.svg"
        )),
        // Like the Layer Manager button: it opens a pallet, and the ribbon drops
        // the highlight once the command has run.
        event: ModuleEvent::Command("LAYWALK".to_string()),
    }
}
