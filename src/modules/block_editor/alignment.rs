use crate::command::{CadCommand, CmdOption, CmdResult, PointPickContext};
use crate::scene::block_alignment::{Alignment, Targets};
use codec::{Document, EntityType, Handle};
use glam::DVec3;

pub struct AlignmentCommand {
    document: u64,
    alignment: Alignment,
    targets: Targets,
    context: Option<PointPickContext>,
    flipped: bool,
    hidden: [Handle; 1],
}

impl AlignmentCommand {
    pub fn new(document: u64, alignment: Alignment, targets: Targets) -> Self {
        let hidden = [alignment.reference.common.handle];
        Self {
            document,
            alignment,
            targets,
            context: None,
            flipped: false,
            hidden,
        }
    }
}

impl CadCommand for AlignmentCommand {
    fn name(&self) -> &'static str {
        "BLOCKALIGNMENTGRIP"
    }
    fn prompt(&self) -> String {
        crate::t!("Specify alignment grip position or [Flip] (Escape cancels):").into_owned()
    }
    fn options(&self) -> Vec<CmdOption> {
        vec![CmdOption::new(crate::t!("Flip").as_ref(), "Flip")]
    }
    fn wants_point_pick_context(&self) -> bool {
        true
    }
    fn set_point_pick_context(&mut self, context: Option<PointPickContext>) {
        self.context = context;
    }
    fn cursor_plane(&self) -> Option<(DVec3, DVec3)> {
        Some((self.alignment.plane.z, self.alignment.anchor))
    }
    fn on_point(&mut self, cursor: DVec3) -> CmdResult {
        if let Err(error) = self
            .targets
            .pose(&self.alignment, cursor, self.context, self.flipped)
        {
            return CmdResult::ReportError(crate::t!(error).into_owned());
        }
        CmdResult::AlignBlockReference {
            document: self.document,
            alignment: Box::new(self.alignment.clone()),
            cursor,
            context: self.context,
            flipped: self.flipped,
        }
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if matches!(text.trim().to_ascii_uppercase().as_str(), "F" | "FLIP") {
            self.flipped = !self.flipped;
            Some(CmdResult::NeedPoint)
        } else {
            None
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn preview_entities(&mut self, _doc: &Document, cursor: DVec3) -> Vec<EntityType> {
        self.targets
            .pose(&self.alignment, cursor, self.context, self.flipped)
            .ok()
            .and_then(|(point, angle)| self.alignment.transformed(point, angle).ok())
            .into_iter()
            .collect()
    }
    fn preview_hidden_handles(&self) -> &[Handle] {
        &self.hidden
    }
}
