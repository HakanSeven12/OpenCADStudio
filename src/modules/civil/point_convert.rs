//! Convert CAD Points (CONVERTCADPOINTS): the drawing's POINT objects become
//! COGO points (measured, CV01): "Select CAD points:" gathers them (other
//! objects are ignored, Enter on nothing ends), then each point is asked its
//! description ("<.>" every time: the default is not kept) and becomes a COGO
//! point at its x, y and z, the POINT removed; then the selection is asked
//! again. The whole command undoes as one step.

use std::collections::VecDeque;

use crate::command::{CadCommand, CmdResult, InputKind};
use crate::modules::draw::select::SelectObjectsCommand;
use document::{EntityType, Handle};
use glam::DVec3;

/// The private command the selection hands its points to.
pub const GATHERED: &str = "_CONVERTCADPOINTS_GATHERED";

/// The POINT selection that opens each round.
pub fn selection() -> SelectObjectsCommand {
    SelectObjectsCommand::filtered("CONVERTCADPOINTS", GATHERED, "Select CAD points:", |e| matches!(e, EntityType::Point(_)))
}

/// The description of each gathered point, in creation order.
pub struct DescribePointsCommand {
    queue: VecDeque<(Handle, DVec3)>,
}

impl DescribePointsCommand {
    pub fn new(mut points: Vec<(Handle, DVec3)>) -> Self {
        points.sort_by_key(|(h, _)| h.value());
        Self { queue: points.into() }
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

impl CadCommand for DescribePointsCommand {
    fn name(&self) -> &'static str {
        "CONVERTCADPOINTS"
    }

    fn prompt(&self) -> String {
        crate::t!("Enter a point description <.>:").into_owned()
    }

    fn input_kind(&self) -> InputKind {
        InputKind::FreeText
    }

    fn groups_undo(&self) -> bool {
        true
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").unwrap_or(CmdResult::NeedPoint)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let (handle, at) = self.queue.pop_front()?;
        let typed = text.trim();
        let description = (!typed.is_empty() && typed != ".").then(|| typed.to_string());
        Some(CmdResult::ConvertCadPoint { handle, at, description, more: !self.queue.is_empty() })
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["CREATEPOINTGROUP", "CREATEPOINTSFROMCORRIDOR", "CREATEBLOCKSFROMPOINTS", "CONVERTCADPOINTS", "REPLACELEGACYPOINTBLOCKS"]
});
