use crate::modules::{IconKind, ModuleEvent, RibbonItem, ToolDef};
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/vports_config.svg"));
pub fn tool() -> ToolDef {
    ToolDef {
        id: "VPORTS",
        label: "Viewport\nConfiguration",
        icon: ICON,
        event: ModuleEvent::Command("VPORTS".to_string()),
    }
}

/// The tiled model viewport layouts, picked from the ribbon.
pub fn dropdown() -> RibbonItem {
    macro_rules! layout {
        ($cmd:literal, $label:literal, $icon:literal) => {
            (
                $cmd,
                $label,
                IconKind::Svg(include_bytes!(concat!("../../../assets/icons/vports_", $icon, ".svg"))),
            )
        };
    }
    RibbonItem::LargeDropdown {
        id: "VPORTS_CONFIG",
        label: "Viewport\nConfiguration",
        icon: ICON,
        items: vec![
            layout!("VPORTS SINGLE", "Single", "single"),
            layout!("VPORTS 2V", "Two: Vertical", "2v"),
            layout!("VPORTS 2H", "Two: Horizontal", "2h"),
            layout!("VPORTS 3R", "Three: Right", "3r"),
            layout!("VPORTS 3L", "Three: Left", "3l"),
            layout!("VPORTS 3A", "Three: Above", "3a"),
            layout!("VPORTS 3B", "Three: Below", "3b"),
            layout!("VPORTS 3V", "Three: Vertical", "3v"),
            layout!("VPORTS 3H", "Three: Horizontal", "3h"),
            layout!("VPORTS 4", "Four: Equal", "4"),
        ],
        default: "VPORTS SINGLE",
    }
}
