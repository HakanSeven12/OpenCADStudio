// Group tool — ribbon definition + CadCommand implementation.

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub fn tool() -> ToolDef {
    crate::modules::ribbon_command("GROUP")
}

// ── CadCommand implementation ─────────────────────────────────────────────

use acadrust::Handle;
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult};
use crate::scene::model::wire_model::WireModel;

pub struct GroupCommand {
    handles: Vec<Handle>,
    /// Auto-generated fallback name shown in the prompt.
    auto_name: String,
}

impl GroupCommand {
    pub fn new(handles: Vec<Handle>, auto_name: String) -> Self {
        Self { handles, auto_name }
    }
}

impl CadCommand for GroupCommand {
    fn name(&self) -> &'static str {
        "GROUP"
    }

    fn prompt(&self) -> String {
        let name = self.auto_name.clone();
        t!("GROUP  Enter group name [%{name}]:", name = name).into_owned()
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let name = if text.trim().is_empty() {
            std::mem::take(&mut self.auto_name)
        } else {
            text.trim().to_string()
        };
        Some(CmdResult::CreateGroup {
            handles: std::mem::take(&mut self.handles),
            name,
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        // Enter with no typed name uses the auto-generated name.
        CmdResult::CreateGroup {
            handles: std::mem::take(&mut self.handles),
            name: std::mem::take(&mut self.auto_name),
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_hover_entity(&mut self, _handle: Handle, _pt: DVec3) -> Vec<WireModel> {
        vec![]
    }
}
