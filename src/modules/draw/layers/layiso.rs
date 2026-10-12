use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    ToolDef {
        id: "LAYISO",
        label: "Isolate Layer",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/layers/layiso.svg")),
        event: ModuleEvent::Command("LAYISO".to_string()),
    }
}
