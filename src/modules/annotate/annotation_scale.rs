use document::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};

/// Selection front-end used by the annotation-scale ribbon actions (and any
/// command that only needs objects before it runs). The action itself runs
/// only after the normal selection engine completes, so window/crossing
/// selection, removal, Escape and Previous all keep their usual behavior.
pub struct AnnotationScaleSelectionCommand {
    name: &'static str,
    action: &'static str,
    annotative: bool,
}

impl AnnotationScaleSelectionCommand {
    pub fn new(name: &'static str, action: &'static str) -> Self {
        Self {
            name,
            action,
            annotative: true,
        }
    }

    /// Ask for any objects, then rerun `name` with them selected.
    pub fn objects(name: &'static str) -> Self {
        Self {
            name,
            action: name,
            annotative: false,
        }
    }
}

impl CadCommand for AnnotationScaleSelectionCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        if self.annotative {
            crate::tf!("{}  Select annotative objects:", self.name).into_owned()
        } else {
            crate::t!("Select objects:").into_owned()
        }
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
