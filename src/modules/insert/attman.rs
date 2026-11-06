use crate::modules::{IconKind, ModuleEvent, ToolDef};
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/attman.svg"));
pub fn tool() -> ToolDef {
    ToolDef {
        id: "ATTMAN",
        label: "Manage",
        icon: ICON,
        event: ModuleEvent::Command("ATTMAN".to_string()),
    }
}

/// The Block Attribute Manager's "Select block" button: one block reference
/// picked in the drawing, handed back as `_ATTMAN_PICK <hex handle>`.
pub struct AttmanPickCommand;

impl crate::command::CadCommand for AttmanPickCommand {
    fn name(&self) -> &'static str {
        "BATTMAN"
    }

    fn prompt(&self) -> String {
        "Select a block:".into()
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn on_entity_pick(
        &mut self,
        handle: document::Handle,
        _pt: glam::DVec3,
    ) -> crate::command::CmdResult {
        if handle.is_null() {
            return crate::command::CmdResult::NeedPoint;
        }
        crate::command::CmdResult::Dispatch(format!("_ATTMAN_PICK {:X}", handle.value()))
    }

    fn on_point(&mut self, _pt: glam::DVec3) -> crate::command::CmdResult {
        crate::command::CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> crate::command::CmdResult {
        crate::command::CmdResult::Dispatch("_ATTMAN_PICK".into())
    }

    fn on_escape(&mut self) -> crate::command::CmdResult {
        crate::command::CmdResult::Dispatch("_ATTMAN_PICK".into())
    }
}
