use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub const ICON: IconKind = IconKind::Svg(include_bytes!(
    "../../../assets/icons/graphic_attributes.svg"
));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "GRAPHICATTRIBUTES",
        label: "Graphic Attributes",
        icon: ICON,
        event: ModuleEvent::Command("GRAPHICATTRIBUTES".to_string()),
    }
}
