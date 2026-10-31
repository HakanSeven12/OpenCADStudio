//! Dragging an arrow grip of an associative array: the array's counts and
//! spacings follow the cursor, the click (or a typed value) commits them.

use crate::command::{CadCommand, CmdResult};
use crate::scene::assoc_array::AssocArray;
use crate::scene::assoc_array_grips::{Drag, Kind};
use crate::scene::model::wire_model::WireModel;
use codec::{EntityType, Handle};
use glam::DVec3;

pub struct ArrayGripCommand {
    pub array: AssocArray,
    pub kind: Kind,
    pub drag: Drag,
    ctrl: bool,
    hidden: [Handle; 1],
}

impl ArrayGripCommand {
    pub fn new(array: AssocArray, kind: Kind, drag: Drag) -> Self {
        let hidden = [array.insert];
        Self {
            array,
            kind,
            drag,
            ctrl: false,
            hidden,
        }
    }

    fn commit(&self, changes: Vec<(&'static str, f64)>) -> CmdResult {
        if changes.is_empty() {
            return CmdResult::NeedPoint;
        }
        CmdResult::EditAssocArray {
            insert: self.array.insert,
            changes,
        }
    }
}

impl CadCommand for ArrayGripCommand {
    fn name(&self) -> &'static str {
        "ARRAYEDIT"
    }

    fn prompt(&self) -> String {
        self.kind.prompt().to_string()
    }

    fn set_ctrl(&mut self, ctrl: bool) {
        self.ctrl = ctrl;
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.commit(self.drag.changes(pt, self.ctrl))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let changes = self.drag.typed(text, self.ctrl)?;
        Some(self.commit(changes))
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let anchor = self.drag.anchor();
        Some(WireModel::solid_f64(
            "array_grip_band".into(),
            vec![anchor.to_array(), pt.to_array()],
            WireModel::CYAN,
            false,
        ))
    }

    fn preview_entities(&mut self, document: &codec::Document, pt: DVec3) -> Vec<EntityType> {
        let changes = self.drag.changes(pt, self.ctrl);
        crate::scene::assoc_array_grips::preview(document, &self.array, &changes)
    }

    fn preview_hidden_handles(&self) -> &[Handle] {
        &self.hidden
    }
}
