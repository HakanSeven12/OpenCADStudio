use crate::command::{CadCommand, CmdResult};
use document::Handle;

inventory::submit!(crate::command::CommandRegistration {
    names: &["RESETBLOCK"]
});

pub(crate) struct ResetCommand {
    handles: Vec<Handle>,
}

impl ResetCommand {
    pub(crate) fn new() -> Self {
        Self { handles: vec![] }
    }
}

impl CadCommand for ResetCommand {
    fn name(&self) -> &'static str {
        "RESETBLOCK"
    }
    fn prompt(&self) -> String {
        crate::t!("RESETBLOCK  Select dynamic block references, then press Enter:").into_owned()
    }
    fn is_selection_gathering(&self) -> bool {
        true
    }
    fn on_point(&mut self, _: glam::DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.handles = handles;
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        if self.handles.is_empty() {
            CmdResult::Cancel
        } else {
            CmdResult::Relaunch("RESETBLOCK APPLY".into(), std::mem::take(&mut self.handles))
        }
    }
}
