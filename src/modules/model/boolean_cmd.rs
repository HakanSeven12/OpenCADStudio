use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, SelectionEntity};

/// Which boolean operation to apply to selected solids.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoolOp {
    Union,
    Subtract,
    Intersect,
}

impl BoolOp {
    pub fn from_id(id: &str) -> Option<BoolOp> {
        Some(match id {
            "UNION" => BoolOp::Union,
            "SUBTRACT" => BoolOp::Subtract,
            "INTERSECT" => BoolOp::Intersect,
            _ => return None,
        })
    }
}

enum SubtractStep {
    Bases,
    Cutters,
}

pub struct SubtractCommand {
    step: SubtractStep,
    bases: Vec<Handle>,
    selected: Vec<Handle>,
}

impl SubtractCommand {
    pub fn new(bases: Vec<Handle>) -> Self {
        let step = if bases.is_empty() {
            SubtractStep::Bases
        } else {
            SubtractStep::Cutters
        };
        Self {
            step,
            bases,
            selected: Vec::new(),
        }
    }
}

impl CadCommand for SubtractCommand {
    fn name(&self) -> &'static str {
        "SUBTRACT"
    }

    fn prompt(&self) -> String {
        match self.step {
            SubtractStep::Bases => crate::t!("SUBTRACT  Select base solids, then press Enter:")
                .into_owned(),
            SubtractStep::Cutters => {
                crate::t!("SUBTRACT  Select solids to subtract, then press Enter:").into_owned()
            }
        }
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.selected.is_empty() {
            return CmdResult::NeedPoint;
        }
        match self.step {
            SubtractStep::Bases => {
                self.bases = std::mem::take(&mut self.selected);
                self.step = SubtractStep::Cutters;
                CmdResult::DeselectAndContinue
            }
            SubtractStep::Cutters => CmdResult::SolidSubtract {
                bases: std::mem::take(&mut self.bases),
                cutters: std::mem::take(&mut self.selected),
            },
        }
    }

    fn is_selection_gathering(&self) -> bool {
        true
    }

    fn inject_selection_entities(&mut self, entities: Vec<SelectionEntity>) {
        self.selected = entities
            .into_iter()
            .filter_map(|entity| {
                matches!(entity.entity, EntityType::Solid3D(_)).then_some(entity.handle)
            })
            .collect();
    }

    fn on_selection_complete(&mut self, _handles: Vec<Handle>) -> CmdResult {
        CmdResult::NeedPoint
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["UNION", "SUBTRACT", "INTERSECT"]
});
