use acadrust::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};

/// Selection front-end used by the annotation-scale ribbon actions. The
/// action itself runs only after the normal selection engine completes, so
/// window/crossing selection, removal, Escape and Previous all keep their
/// usual behavior.
pub struct AnnotationScaleSelectionCommand {
    name: &'static str,
    action: &'static str,
}

impl AnnotationScaleSelectionCommand {
    pub fn new(name: &'static str, action: &'static str) -> Self {
        Self { name, action }
    }
}

impl CadCommand for AnnotationScaleSelectionCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        crate::tf!("{}  Select annotative objects:", self.name).into_owned()
    }

    fn is_selection_gathering(&self) -> bool {
        true
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if handles.is_empty() {
            CmdResult::Cancel
        } else {
            CmdResult::Relaunch(self.action.to_string(), handles)
        }
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
