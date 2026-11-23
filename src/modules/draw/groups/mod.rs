pub mod group;
pub mod ungroup;
pub mod groupedit;

use crate::modules::{IconKind, ModuleEvent, ToolDef};

/// GROUPEDIT: add objects to a group, remove members or rename it.
pub fn edit_tool() -> ToolDef {
    ToolDef {
        id: "GROUPEDIT",
        label: "Edit Group",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/group_edit.svg")),
        event: ModuleEvent::Command("GROUPEDIT".to_string()),
    }
}

/// Group selection on and off (PICKSTYLE), lit while picks take whole
/// groups; the same switch as Ctrl+Shift+A and Ctrl+H.
pub fn selection_tool() -> ToolDef {
    ToolDef {
        id: "TOGGLEGROUPS",
        label: "Group Selection",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/group_select.svg")),
        event: ModuleEvent::Command("TOGGLEGROUPS".to_string()),
    }
}
