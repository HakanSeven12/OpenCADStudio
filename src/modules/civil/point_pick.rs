//! The picks the Create Points tool windows step aside for: a corridor when
//! the drawing holds several, and the window of Create Blocks from COGO
//! Points' "Define window".

use crate::command::{CadCommand, CmdResult, InputKind};
use document::{EntityType, Handle};
use glam::DVec3;

/// Hands the picked corridor's handle (hex) to the corridor points window.
pub const CORRIDOR_PICKED: &str = "_CORRIDORPOINTS_PICKED";
/// Hands the window's corners (`x1 y1 x2 y2`) back to the blocks window;
/// alone, the window comes back unchanged.
pub const WINDOW_PICKED: &str = "_POINTBLOCKS_WINDOW";

/// "Select a corridor:" of Create COGO Points from Corridor.
#[derive(Default)]
pub struct CorridorPickCommand {
    picked: Option<bool>,
}

impl CadCommand for CorridorPickCommand {
    fn name(&self) -> &'static str {
        "CREATEPOINTSFROMCORRIDOR"
    }

    fn prompt(&self) -> String {
        crate::t!("Select a corridor:").into_owned()
    }

    fn input_kind(&self) -> InputKind {
        InputKind::Point
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(matches!(&entity, EntityType::Extended(x) if matches!(&x.data, document::entities::ExtendedEntityData::Arch(a) if a.dxf_name == "AECC_CORRIDOR")));
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        match self.picked.take() {
            Some(true) => CmdResult::Dispatch(format!("{CORRIDOR_PICKED} {:X}", handle.value())),
            _ => CmdResult::ReportError(crate::t!("Select a corridor.").into_owned()),
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// The two corners of "Define window".
#[derive(Default)]
pub struct WindowPickCommand {
    first: Option<DVec3>,
}

impl CadCommand for WindowPickCommand {
    fn name(&self) -> &'static str {
        "CREATEBLOCKSFROMPOINTS"
    }

    fn prompt(&self) -> String {
        match self.first {
            None => crate::t!("Specify first corner:"),
            Some(_) => crate::t!("Specify opposite corner:"),
        }
        .into_owned()
    }

    fn input_kind(&self) -> InputKind {
        InputKind::Point
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.first.take() {
            None => {
                self.first = Some(pt);
                CmdResult::NeedPoint
            }
            Some(a) => CmdResult::Dispatch(format!("{WINDOW_PICKED} {} {} {} {}", a.x, a.y, pt.x, pt.y)),
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Dispatch(WINDOW_PICKED.into())
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Dispatch(WINDOW_PICKED.into())
    }
}
