use acadrust::entities::Underlay;
use acadrust::types::{Handle, Vector3};
use acadrust::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::t;

pub struct PdfAttachCommand {
    definition_handle: Handle,
    plane: WorkingPlane,
}

impl PdfAttachCommand {
    pub fn new(definition_handle: Handle) -> Self {
        Self {
            definition_handle,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for PdfAttachCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "PDFATTACH"
    }

    fn prompt(&self) -> String {
        t!("PDFATTACH  Specify insertion point:").into_owned()
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let point = self.plane.to_local(pt);

        let mut underlay = Underlay::pdf();
        underlay.definition_handle = self.definition_handle;
        underlay.insertion_point = Vector3::new(point.x, point.y, point.z);

        CmdResult::CommitAndExit(
            self.plane
                .place_entity(EntityType::Underlay(underlay)),
        )
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["PDFATTACH"]
});
